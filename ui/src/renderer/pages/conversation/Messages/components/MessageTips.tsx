

import type { IMessageTips } from '@/common/chat/chatLib';
import { ipcBridge } from '@/common';
import { isBackendHttpError } from '@/common/adapter/httpBridge';
import { toDisplayText } from '@/common/chat/displayText';
import { Button, Collapse } from '@arco-design/web-react';
import { Attention, CheckOne } from '@icon-park/react';
import { theme } from '@/platform';
import { AppMessage as Message } from '@/renderer/components/notifications';
import classNames from 'classnames';
import React, { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import MarkdownView from '@renderer/components/Markdown';
import FeedbackButton from '@renderer/components/base/FeedbackButton';
import CopyIconButton from '@renderer/components/base/CopyIconButton';
import CollapsibleContent from '@renderer/components/chat/CollapsibleContent';
import { emitter } from '@/renderer/utils/emitter';
import { useConversationContextSafe } from '@/renderer/hooks/context/ConversationContext';
import { useMessageList } from '../hooks';
import { parseMessageFileMarker } from './messageFileMarker';
import { resolveMessageErrorRecoveryAction } from './messageErrorRecovery';
import { MESSAGE_BODY_FONT_SIZE, MESSAGE_BODY_LINE_HEIGHT } from '../typography';
import { useNavigate } from 'react-router-dom';
import { trackFunnelEvent } from '@/renderer/utils/analytics/productFunnel';
import type { ConversationErrorReportContext } from '@/renderer/features/supportChat/conversationErrorReport';
import {
  buildAgentErrorDiagnostic,
  buildErrorDiagnostic,
  formatErrorDiagnosticText,
  getErrorDiagnosticLabels,
} from '@/renderer/utils/ui/errorDiagnostics';

const icon = {
  success: <CheckOne theme='filled' size='16' fill={theme.Color.FunctionalColor.success} className='m-t-2px' />,
  warning: (
    <Attention
      theme='filled'
      size='16'
      strokeLinejoin='bevel'
      className='m-t-2px'
      fill={theme.Color.FunctionalColor.warn}
    />
  ),
  error: (
    <Attention
      theme='filled'
      size='16'
      strokeLinejoin='bevel'
      className='m-t-2px'
      fill='currentColor'
    />
  ),
};

const useFormatContent = (content: string) => {
  return useMemo(() => {
    try {
      const json = JSON.parse(content);
      return {
        json: true,
        data: json,
      };
    } catch {
      return { data: content };
    }
  }, [content]);
};

/**
 * Retry entry for a failed turn: recalls the originating user request into
 * the composer via the shared `sendbox.edit` channel (edit mode: submitting
 * truncates and reruns). Only offered on the nomi surface, for errors that
 * answer the latest user request, once the turn has settled.
 */
const useErrorEdit = (message: IMessageTips): (() => void) | null => {
  const conversationContext = useConversationContextSafe();
  const messageList = useMessageList();
  return useMemo(() => {
    if (message.content.type !== 'error') return null;
    if (message.content.recovery) return null;
    if (message.content.error?.retryable === false) return null;
    if (conversationContext?.type !== 'nomi') return null;
    if (conversationContext.readOnly === true) return null;
    if (conversationContext.isProcessing === true) return null;
    const lastRight = messageList.findLast((entry) => entry.type === 'text' && entry.position === 'right');
    if (!lastRight || lastRight.type !== 'text') return null;
    const retryMessageId = lastRight.message_id ?? lastRight.msg_id;
    const retryCreatedAt = lastRight.created_at;
    if (!retryMessageId || retryCreatedAt == null) return null;
    if ((message.created_at ?? 0) < retryCreatedAt) return null;
    const rawContent = typeof lastRight.content?.content === 'string' ? lastRight.content.content : '';
    const { text } = parseMessageFileMarker(rawContent, 'right');
    if (!text.trim()) return null;
    return () => emitter.emit('sendbox.edit', { msgId: retryMessageId, createdAt: retryCreatedAt, content: text });
  }, [conversationContext, message.content, message.created_at, messageList]);
};

type ContinueState = 'idle' | 'pending' | 'accepted' | 'stale';

const useTruncatedContinuation = (message: IMessageTips) => {
  const { t } = useTranslation();
  const conversationContext = useConversationContextSafe();
  const [state, setState] = useState<ContinueState>('idle');
  const recovery = message.content.recovery;
  const expectedUiErrorCode =
    recovery?.failure_code === 'output_truncated'
      ? 'OUTPUT_TRUNCATED'
      : recovery?.failure_code === 'turn_requests_exhausted'
        ? 'TURN_REQUESTS_EXHAUSTED'
        : undefined;
  const visible = Boolean(
    message.content.type === 'error' &&
      message.content.error?.retryable === true &&
      recovery &&
      message.content.error.code === expectedUiErrorCode &&
      conversationContext?.type === 'nomi' &&
      conversationContext.readOnly !== true
  );
  const disabled =
    !visible || conversationContext?.isProcessing === true || state === 'pending' || state === 'accepted' || state === 'stale';

  const continueTurn = useCallback(async () => {
    if (!recovery || !conversationContext || disabled) return;
    setState('pending');
    try {
      await ipcBridge.conversation.continueTruncated.invoke({
        conversation_id: conversationContext.conversation_id,
        source_message_id: recovery.source_message_id,
        // One source failure owns exactly one continuation operation. The
        // stable key absorbs double-clicks, transport retries, and remounts.
        idempotency_key: recovery.source_message_id,
      });
      setState('accepted');
    } catch (error) {
      if (isBackendHttpError(error) && error.status === 409) {
        setState('stale');
        Message.warning(
          t('conversation.truncation.stale', {
            defaultValue: 'This interrupted turn has already been superseded.',
          })
        );
        return;
      }
      setState('idle');
      Message.error(
        t('conversation.truncation.failed', {
          defaultValue: 'Could not continue the interrupted turn.',
        })
      );
    }
  }, [conversationContext, disabled, recovery, t]);

  const label =
    state === 'pending'
      ? t('conversation.truncation.continuing', { defaultValue: 'Continuing…' })
      : state === 'accepted'
        ? t('conversation.truncation.accepted', { defaultValue: 'Continuation started' })
        : state === 'stale'
          ? t('conversation.truncation.superseded', { defaultValue: 'Already superseded' })
          : t('conversation.truncation.continue', { defaultValue: 'Continue execution' });

  return { visible, disabled, label, continueTurn };
};

const MessageTips: React.FC<{ message: IMessageTips }> = ({ message }) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const messageList = useMessageList();
  const { type } = message.content;
  const content = toDisplayText(message.content.content);
  const structuredError = type === 'error' ? message.content.error : undefined;
  const { json, data } = useFormatContent(content);
  const edit = useErrorEdit(message);
  const continuation = useTruncatedContinuation(message);
  const editButton = edit ? (
    <button type='button' className='message-error-note__retry' data-testid='message-error-edit' onClick={edit}>
      {t('common.edit', { defaultValue: 'Edit' })}
    </button>
  ) : null;
  const continueButton = continuation.visible ? (
    <button
      type='button'
      className='message-error-note__retry'
      data-testid='message-error-continue-truncated'
      disabled={continuation.disabled}
      onClick={() => void continuation.continueTurn()}
    >
      {continuation.label}
    </button>
  ) : null;

  const displayContent = json ? '' : content;
  const conversationErrorReport = useMemo<ConversationErrorReportContext | undefined>(() => {
    if (type !== 'error') return undefined;
    return {
      error: structuredError ?? { message: content },
      conversationId: message.conversation_id,
      ...(message.message_id || message.msg_id ? { messageId: message.message_id ?? message.msg_id } : {}),
      ...(message.turn_id ? { turnId: message.turn_id } : {}),
      occurredAt: new Date(message.created_at ?? Date.now()).toISOString(),
    };
  }, [
    content,
    message.conversation_id,
    message.created_at,
    message.message_id,
    message.msg_id,
    message.turn_id,
    structuredError,
    type,
  ]);

  const retryPayload = useMemo(() => {
    if (type !== 'error') return null;
    if (structuredError && structuredError.retryable === false) return null;
    const tipIndex = messageList.findIndex((item) => item.id === message.id);
    const end = tipIndex === -1 ? messageList.length : tipIndex;
    for (let i = end - 1; i >= 0; i -= 1) {
      const item = messageList[i];
      if (item.type !== 'text' || item.position !== 'right') continue;
      const raw = typeof item.content?.content === 'string' ? item.content.content : '';
      const { text } = parseMessageFileMarker(raw, 'right');
      const trimmed = text.trim();
      if (!trimmed) continue;
      return {
        content: trimmed,
        msgId: item.msg_id,
        createdAt: item.created_at,
      };
    }
    return null;
  }, [message.id, messageList, structuredError, type]);

  const recoveryAction = useMemo(
    () => resolveMessageErrorRecoveryAction(structuredError),
    [structuredError?.code, structuredError?.resolution?.kind]
  );
  const recoveryActionLabel = recoveryAction
    ? t(recoveryAction.labelKey, {
        defaultValue:
          recoveryAction.source === 'open_billing'
            ? 'Buy credits'
            : recoveryAction.source === 'change_model'
              ? 'Change model'
              : 'Fix setup',
      })
    : null;

  if (type === 'error') {
    const diagnostic = structuredError
      ? buildAgentErrorDiagnostic(structuredError)
      : buildErrorDiagnostic({ message: content, detail: json ? content : undefined });
    const diagnosticSummary = diagnostic.summary || t('conversation.agentError.unknownError');
    const diagnosticText = formatErrorDiagnosticText(
      diagnostic.summary ? diagnostic : { ...diagnostic, summary: diagnosticSummary },
      getErrorDiagnosticLabels((key) => t(key))
    );
    const code = structuredError?.code;
    const ownership = structuredError?.ownership;
    const title = code
      ? t(`conversation.agentError.codes.${code}.title`, {
          defaultValue: t('conversation.agentError.fallbackTitle'),
        })
      : t('conversation.agentError.fallbackTitle');
    const body = code
      ? t(
          `conversation.agentError.codes.${code}.body`,
          {
            defaultValue: diagnosticSummary,
          }
        )
      : diagnosticSummary;
    const ownershipLabel = ownership
      ? t(`conversation.agentError.ownership.${ownership}`, {
          defaultValue: t('conversation.agentError.ownership.unknown_upstream'),
        })
      : null;
    const resolutionText = structuredError?.resolution
      ? t(`conversation.agentError.resolution.${structuredError.resolution.kind}`)
      : null;
    const shouldShowResolution = Boolean(resolutionText && !retryPayload && !recoveryAction);
    return (
      <div className='w-full'>
        <div className={classNames('message-error-note', ownership && `message-error-note--${ownership}`)}>
          <div className='message-error-note__content'>
            <div className='message-error-note__header'>
              <div className='message-error-note__status'>
                <span className='message-error-note__icon'>{icon.error}</span>
                {ownershipLabel && <span className='message-error-note__owner'>{ownershipLabel}</span>}
              </div>
              <div className='message-error-note__meta'>
                {code && (
                  <div className='message-error-note__meta-row'>
                    <span className='message-error-note__meta-label'>{t('conversation.agentError.errorCode')}</span>
                    <code className='message-error-note__code' title={code}>
                      {code}
                    </code>
                  </div>
                )}
                {diagnostic.modelId && (
                  <div className='message-error-note__meta-row'>
                    <span className='message-error-note__meta-label'>{t('conversation.agentError.modelId')}</span>
                    <code className='message-error-note__model' title={diagnostic.modelId}>
                      {diagnostic.modelId}
                    </code>
                  </div>
                )}
              </div>
            </div>
            <div className='message-error-note__main'>
              <div className='message-error-note__title'>{title}</div>
              <div className='message-error-note__body'>{body}</div>
              <div className='message-error-note__diagnostic-summary' role='status'>
                <span className='message-error-note__diagnostic-label'>
                  {t('conversation.agentError.diagnosticSummary')}
                </span>
                <span className='message-error-note__diagnostic-text'>{diagnosticSummary}</span>
              </div>
              {shouldShowResolution && (
                <div className='message-error-note__resolution'>
                  <span className='message-error-note__resolution-label'>
                    {t('conversation.agentError.resolutionPrefix')}
                  </span>
                  <span>{resolutionText}</span>
                </div>
              )}
              <div className='message-error-note__footer'>
                <div className='message-error-note__footer-main'>
                  {diagnosticText.length > 0 && (
                    <Collapse bordered={false} className='message-error-note__details'>
                      <Collapse.Item
                        name='technical-details'
                        header={
                          <span className='message-error-note__details-label'>{t('common.technical_details')}</span>
                        }
                      >
                        <pre className='message-error-note__detail-body'>{diagnosticText}</pre>
                      </Collapse.Item>
                    </Collapse>
                  )}
                  <div className='message-error-note__actions'>
                    {continueButton}
                    {!continuation.visible && retryPayload && (
                      <Button
                        size='mini'
                        type='primary'
                        className='message-error-note__retry'
                        onClick={() => {
                          emitter.emit('sendbox.retry', {
                            content: retryPayload.content,
                            ...(retryPayload.msgId ? { msgId: retryPayload.msgId } : {}),
                            ...(retryPayload.createdAt ? { createdAt: retryPayload.createdAt } : {}),
                          });
                        }}
                      >
                        {t('conversation.agentError.retryAction', { defaultValue: 'Retry' })}
                      </Button>
                    )}
                    {!continuation.visible && !retryPayload && recoveryAction && (
                      <Button
                        size='mini'
                        className='message-error-note__recovery'
                        data-testid='message-error-recovery'
                        onClick={() => {
                          trackFunnelEvent('prerequisite_resolved', {
                            kind: recoveryAction.source,
                            source: 'error_recovery',
                          });
                          navigate(recoveryAction.href);
                        }}
                      >
                        {recoveryActionLabel}
                      </Button>
                    )}
                    {editButton}
                    <CopyIconButton
                      text={diagnosticText}
                      tooltip={t('conversation.agentError.copyDiagnostic')}
                      successMessage={t('conversation.agentError.copyDiagnosticSuccess')}
                      className='message-error-note__copy'
                    />
                    <FeedbackButton
                      conversationErrorReport={conversationErrorReport}
                      className='message-error-note__feedback'
                    />
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    );
  }

  if (json)
    return (
      <div className='w-full'>
        <div className={classNames('bg-message-tips rd-8px p-x-12px p-y-8px flex flex-col gap-4px')}>
          <div className='flex items-start gap-4px'>
            {icon[type] || icon.warning}
            <div className='flex-1 min-w-0'>
              <MarkdownView fontSize={MESSAGE_BODY_FONT_SIZE} lineHeight={MESSAGE_BODY_LINE_HEIGHT}>
                {`\`\`\`json\n${JSON.stringify(data, null, 2)}\n\`\`\``}
              </MarkdownView>
            </div>
          </div>
        </div>
      </div>
    );
  return (
    <div className='w-full'>
      <div className={classNames('bg-message-tips rd-8px  p-x-12px p-y-8px flex flex-col gap-4px')}>
        <div className='flex items-start gap-4px'>
          {icon[type] || icon.warning}
          <div className='flex-1 min-w-0'>
            <CollapsibleContent maxHeight={48} defaultCollapsed={true} useMask={true}>
              <span className='whitespace-break-spaces text-t-primary [word-break:break-word]'>{displayContent}</span>
            </CollapsibleContent>
          </div>
        </div>
      </div>
    </div>
  );
};

export default MessageTips;
