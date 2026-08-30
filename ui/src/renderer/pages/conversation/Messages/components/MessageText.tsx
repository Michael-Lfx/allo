

import { preferKnowledgeWritebackState, type IMessageText, type KnowledgeWritebackState, type KnowledgeWritebackStatus } from '@/common/chat/chatLib';
import { ipcBridge } from '@/common';
import { toDisplayText } from '@/common/chat/displayText';
import type { TurnCreditUsageData } from '@/common/config/storage';
import type { MessageId } from '@/common/types/ids';
import { useConversationContextSafe } from '@/renderer/hooks/context/ConversationContext';
import { iconColors } from '@/renderer/styles/colors';
import { Alert, Button, Modal, Tooltip } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { CheckOne, CloseOne, Copy, Edit, Info, Loading, Undo } from '@icon-park/react';
import classNames from 'classnames';
import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import useSWR from 'swr';
import { copyText } from '@/renderer/utils/ui/clipboard';
import { emitter } from '@/renderer/utils/emitter';
import { useMessageList } from '../hooks';
import { useEditingMessage } from '../editingMessageStore';
import CollapsibleContent from '@renderer/components/chat/CollapsibleContent';
import FilePreview from '@renderer/components/media/FilePreview';
import HorizontalFileList from '@renderer/components/media/HorizontalFileList';
import MarkdownView from '@renderer/components/Markdown';
import CodeBlock from '@renderer/components/beautifulUi/codeBlock/CodeBlock';
import StreamingText from '@renderer/components/beautifulUi/streamingText/StreamingText';
import { stripThinkTags, hasThinkTags } from '@renderer/utils/chat/thinkTagFilter';
import { hasMemCitations, stripMemCitations } from '@renderer/utils/chat/memCitationFilter';
import { stripSkillSuggest, hasSkillSuggest } from '@renderer/utils/chat/skillSuggestParser';
import { MESSAGE_BODY_CLASS_NAME, MESSAGE_BODY_FONT_SIZE, MESSAGE_BODY_LINE_HEIGHT } from '../typography';
import { parseMessageFileMarker } from './messageFileMarker';
import { confirmFirstValue } from '@/renderer/utils/analytics/productFunnel';
import { markFirstWinCompleted } from '@/renderer/utils/onboarding/firstWinMode';
import { getConversationOrNull } from '@/renderer/pages/conversation/utils/conversationCache';
import {
  fetchAndPersistTurnCredits,
  peekTurnCredits,
} from '@/renderer/pages/conversation/platforms/nomi/fetchTurnCredits';
import { useProvidersQuery } from '@/renderer/hooks/agent/useModelProviderList';
import MessageCronBadge from './MessageCronBadge';
import { getAgentLogo } from '@/renderer/utils/model/agentLogo';
import { findProviderById } from '@/renderer/utils/model/cloudModelLabel';
import AgentMessageAvatar from './AgentMessageAvatar';
import { splitStreamingMarkdown } from './streamingMarkdown';
import { formatTurnCreditDetails, formatTurnCreditModels } from './turnCreditsLabel';

const BUBBLE_ENTER_FRESH_MS = 1500;

/**
 * Format a timestamp for message display using locale-aware formatting.
 */
export const formatMessageTime = (timestamp: number, locale?: string): string => {
  const date = new Date(timestamp);
  const now = new Date();
  const resolvedLocale = locale ?? (typeof navigator !== 'undefined' ? navigator.language : 'en-US');
  const sameDay =
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate();

  if (sameDay) {
    return new Intl.DateTimeFormat(resolvedLocale, { hour: '2-digit', minute: '2-digit' }).format(date);
  }

  return new Intl.DateTimeFormat(resolvedLocale, {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(date);
};

export const formatMessageTimeIso = (timestamp: number): string => new Date(timestamp).toISOString();

const CODE_STYLE = { marginTop: 4, marginBlock: 4 };

const RUNNING_WRITEBACK_STATUSES = new Set<KnowledgeWritebackStatus>(['started', 'extracting', 'writing']);
const SUCCESS_WRITEBACK_STATUSES = new Set<KnowledgeWritebackStatus>(['written']);
const WARNING_WRITEBACK_STATUSES = new Set<KnowledgeWritebackStatus>(['partial', 'interrupted']);
const FAILURE_WRITEBACK_STATUSES = new Set<KnowledgeWritebackStatus>(['failed']);

const compactWritebackFiles = (state: KnowledgeWritebackState): string | undefined => {
  const paths = (state.written ?? [])
    .map((file) => file.rel_path)
    .filter((path): path is string => Boolean(path));
  if (!paths.length) return undefined;
  const visible = paths.slice(0, 2).join(', ');
  return paths.length > 2 ? `${visible} +${paths.length - 2}` : visible;
};

const getWritebackTextKey = (status: KnowledgeWritebackStatus): string => {
  switch (status) {
    case 'started':
      return 'messages.knowledgeWriteback.started';
    case 'extracting':
      return 'messages.knowledgeWriteback.extracting';
    case 'writing':
      return 'messages.knowledgeWriteback.writing';
    case 'written':
      return 'messages.knowledgeWriteback.written';
    case 'partial':
      return 'messages.knowledgeWriteback.partial';
    case 'failed':
      return 'messages.knowledgeWriteback.failed';
    case 'no_candidate':
      return 'messages.knowledgeWriteback.noCandidate';
    case 'no_completer':
      return 'messages.knowledgeWriteback.noCompleter';
    case 'disabled':
      return 'messages.knowledgeWriteback.disabled';
    case 'interrupted':
      return 'messages.knowledgeWriteback.interrupted';
  }
};

const MessageKnowledgeWriteback: React.FC<{
  state: KnowledgeWritebackState;
  conversationId: IMessageText['conversation_id'];
  messageId?: IMessageText['message_id'];
  onSettled?: () => void;
}> = ({ state, conversationId, messageId, onSettled }) => {
  const { t } = useTranslation();
  const [retrying, setRetrying] = useState(false);
  const [watchingRetry, setWatchingRetry] = useState(false);
  const [polledState, setPolledState] = useState<KnowledgeWritebackState>();
  const retryFromAttemptRef = useRef<string | undefined>(undefined);
  const sawRunningRef = useRef(RUNNING_WRITEBACK_STATUSES.has(state.status));
  const displayState = useMemo(
    () => preferKnowledgeWritebackState(state, polledState) ?? state,
    [polledState, state]
  );
  const failureCount = displayState.failures?.length ?? 0;
  const writtenCount = displayState.written?.length ?? 0;
  const firstFailure = displayState.failures?.find((failure) => failure.error)?.error;
  const fileSummary = compactWritebackFiles(displayState);
  const detail = firstFailure ?? fileSummary;
  const canRetry =
    displayState.retryable === true &&
    !RUNNING_WRITEBACK_STATUSES.has(displayState.status) &&
    Boolean(messageId);

  useEffect(() => {
    if (
      retrying &&
      (RUNNING_WRITEBACK_STATUSES.has(displayState.status) ||
        displayState.attempt_id !== retryFromAttemptRef.current)
    ) {
      setRetrying(false);
    }
    if (
      watchingRetry &&
      displayState.attempt_id !== retryFromAttemptRef.current &&
      !RUNNING_WRITEBACK_STATUSES.has(displayState.status)
    ) {
      setWatchingRetry(false);
    }
  }, [
    displayState.attempt_id,
    displayState.status,
    retrying,
    watchingRetry,
  ]);

  useEffect(() => {
    if (RUNNING_WRITEBACK_STATUSES.has(displayState.status)) {
      sawRunningRef.current = true;
      return;
    }
    if (!sawRunningRef.current) return;
    sawRunningRef.current = false;
    onSettled?.();
  }, [displayState.status, onSettled]);

  // Realtime fan-out is bounded and may drop a frame without disconnecting.
  // Poll this exact durable owner row while it is running (or while a manual
  // retry is crossing the HTTP/event gap), including old messages outside the
  // newest keyset-history window.
  useEffect(() => {
    if (
      !messageId ||
      (!watchingRetry &&
        !RUNNING_WRITEBACK_STATUSES.has(displayState.status))
    ) {
      return;
    }
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      try {
        const message =
          await ipcBridge.database.getConversationMessage.invoke({
            conversation_id: conversationId,
            message_id: messageId,
          });
        if (
          message.type === 'text' &&
          typeof message.content === 'object' &&
          message.content !== null
        ) {
          const incoming = (message.content as IMessageText['content'])
            .knowledge_writeback;
          if (incoming && !stopped) {
            setPolledState((existing) =>
              preferKnowledgeWritebackState(existing, incoming)
            );
          }
        }
      } catch (error) {
        if (!stopped) {
          console.warn(
            '[MessageKnowledgeWriteback] Failed to refresh durable write-back state:',
            error
          );
        }
      } finally {
        if (!stopped) timer = setTimeout(poll, 1_000);
      }
    };
    timer = setTimeout(poll, 250);
    return () => {
      stopped = true;
      if (timer) clearTimeout(timer);
    };
  }, [
    conversationId,
    displayState.status,
    messageId,
    watchingRetry,
  ]);

  const handleRetry = async () => {
    if (!messageId || retrying) return;
    retryFromAttemptRef.current = displayState.attempt_id;
    setRetrying(true);
    setWatchingRetry(true);
    try {
      await ipcBridge.conversation.retryKnowledgeWriteback.invoke({
        conversation_id: conversationId,
        message_id: messageId,
        attempt_id: displayState.attempt_id ?? '',
      });
      Message.success(t('messages.knowledgeWriteback.retryStarted'));
      // Keep the action disabled until the fresh attempt arrives. This closes
      // the response/event gap without inventing a frontend job state.
    } catch (error) {
      setRetrying(false);
      setWatchingRetry(false);
      Message.error(
        error instanceof Error && error.message
          ? error.message
          : t('messages.knowledgeWriteback.retryFailed')
      );
    }
  };

  // 只有 running/未知这两支用的 `border-line` 是死类名（theme 无 line 键），改成
  // border-arco-2；四支共用的 border-style 由容器统一补，见下面的 border-solid。
  // `border-line` names no colour; the shared style lives on the container.
  const toneClass = RUNNING_WRITEBACK_STATUSES.has(displayState.status)
    ? 'border-arco-2 bg-fill-1 text-t-secondary'
    : SUCCESS_WRITEBACK_STATUSES.has(displayState.status)
      ? 'border-success-4 bg-success-light-1 text-success-6'
      : WARNING_WRITEBACK_STATUSES.has(displayState.status)
        ? 'border-warning-3 bg-warning-1 text-warning-7'
        : FAILURE_WRITEBACK_STATUSES.has(displayState.status)
          ? 'border-danger-4 bg-danger-light-1 text-danger-6'
          : 'border-arco-2 bg-fill-1 text-t-secondary';

  const icon = RUNNING_WRITEBACK_STATUSES.has(displayState.status) ? (
    <Loading theme='outline' size='13' className='block shrink-0 animate-spin' />
  ) : SUCCESS_WRITEBACK_STATUSES.has(displayState.status) ? (
    <CheckOne theme='filled' size='13' className='block shrink-0' />
  ) : FAILURE_WRITEBACK_STATUSES.has(displayState.status) ? (
    <CloseOne theme='filled' size='13' className='block shrink-0' />
  ) : (
    <Info theme='outline' size='13' className='block shrink-0' />
  );

  return (
    <div
      className={classNames(
        // border-solid 不能省：本仓库唯一的 preflight 是 `color: inherit`，没有全局
        // border reset，border-style 停在初始值 none —— 四种状态的边框此前全都没画出来。
        // Without an explicit style the badge border never painted in any tone.
        'mt-6px inline-flex max-w-full items-center gap-6px rd-6px border border-solid px-8px py-4px text-12px leading-18px',
        toneClass
      )}
      title={detail}
    >
      <span className='flex h-14px w-14px shrink-0 items-center justify-center self-center leading-none'>{icon}</span>
      <span className='min-w-0 truncate'>
        {t(getWritebackTextKey(displayState.status), {
          count: writtenCount,
          failures: failureCount,
        })}
      </span>
      {detail && <span className='min-w-0 truncate opacity-75'>{detail}</span>}
      {canRetry && (
        <button
          type='button'
          className='ml-2px shrink-0 rd-4px border-0 bg-transparent px-4px py-0 text-inherit font-medium hover:bg-fill-2 disabled:cursor-wait disabled:opacity-60'
          disabled={retrying}
          aria-label={t('messages.knowledgeWriteback.retry')}
          onClick={(event) => {
            event.stopPropagation();
            void handleRetry();
          }}
        >
          {retrying
            ? t('messages.knowledgeWriteback.retrying')
            : t('messages.knowledgeWriteback.retry')}
        </button>
      )}
    </div>
  );
};

const isAbsoluteMessageFilePath = (file_path: string): boolean =>
  file_path.startsWith('/') || /^[A-Za-z]:/.test(file_path);

export const resolveMessageFilePath = (file_path: string, workspace?: string): string => {
  if (!file_path || isAbsoluteMessageFilePath(file_path) || !workspace) {
    return file_path;
  }

  const normalizedWorkspace = workspace.replace(/[\\/]+$/, '').replace(/\\/g, '/');
  const normalizedFilePath = file_path.replace(/^\.?[\\/]+/, '').replace(/\\/g, '/');
  return `${normalizedWorkspace}/${normalizedFilePath}`.replace(/\/+/g, '/');
};

const useFormatContent = (content: string, isStreaming = false) => {
  return useMemo(() => {
    if (isStreaming) {
      return { data: content };
    }
    try {
      const json = JSON.parse(content);
      const isJson = typeof json === 'object';
      return {
        json: isJson,
        data: isJson ? json : content,
      };
    } catch {
      return { data: content };
    }
  }, [content, isStreaming]);
};

const MessageText: React.FC<{
  message: IMessageText;
  hideActions?: boolean;
  actionsOnly?: boolean;
  isStreaming?: boolean;
  /** Prefer list-assigned turn id (turn_actions) over message.msg_id for credit lookup. */
  creditTurnId?: MessageId;
}> = ({ message, hideActions = false, actionsOnly = false, isStreaming = false, creditTurnId }) => {
  // Filter think tags from content before rendering
  // 在渲染前过滤 think 标签
  const contentToRender = useMemo(() => {
    let content = toDisplayText(message.content.content);
    if (hasThinkTags(content)) {
      content = stripThinkTags(content);
    }
    if (hasMemCitations(content)) {
      content = stripMemCitations(content);
    }
    // Strip any inline [SKILL_SUGGEST] blocks (now handled via separate skill_suggest message type)
    if (hasSkillSuggest(content)) {
      content = stripSkillSuggest(content);
    }
    return content;
  }, [message.content.content]);

  const { text, files } = parseMessageFileMarker(contentToRender, message.position);
  const { data, json } = useFormatContent(text, isStreaming);
  const streamingParts = useMemo(
    () => (isStreaming && !json && typeof data === 'string' ? splitStreamingMarkdown(data) : undefined),
    [data, isStreaming, json]
  );
  const { t } = useTranslation();
  const [showCopyAlert, setShowCopyAlert] = useState(false);
  const isUserMessage = message.position === 'right';
  const isAgentMessage = message.position === 'left' && message.content.agentMessage === true;
  const writebackState = !isUserMessage ? message.content.knowledge_writeback : undefined;
  const shouldRenderPlainText = isUserMessage;
  const conversationContext = useConversationContextSafe();
  const shouldShowActions = !hideActions;

  const turnCreditKey =
    creditTurnId ?? message.turn_id ?? message.msg_id ?? message.message_id;
  const conversationId = conversationContext?.conversation_id;
  // Conversation page already hydrates this SWR key — reuse it for persisted credits.
  const { data: conversation } = useSWR(
    conversationId ? `conversation/${conversationId}` : null,
    () => getConversationOrNull(conversationId!)
  );
  const [liveTurnCredits, setLiveTurnCredits] = useState<TurnCreditUsageData | null>(() => {
    if (!conversationId || !turnCreditKey) return null;
    return peekTurnCredits(conversationId, turnCreditKey);
  });
  const persistedTurnCredits = useMemo(() => {
    if (!turnCreditKey || !conversation || conversation.type !== 'nomi') return null;
    return conversation.extra?.turn_credit_usage?.[String(turnCreditKey)] ?? null;
  }, [conversation, turnCreditKey]);
  useEffect(() => {
    if (!turnCreditKey || !conversationId) {
      setLiveTurnCredits(null);
      return;
    }
    // Remount / turn-key change: restore from module cache before the next emit
    // (first-turn list reconcile often remounts MessageText after finish emit).
    setLiveTurnCredits(peekTurnCredits(conversationId, turnCreditKey));
    const handler = (payload: {
      conversation_id: NonNullable<typeof conversationId>;
      turn_id: NonNullable<typeof turnCreditKey>;
      usage: TurnCreditUsageData;
    }) => {
      if (payload.conversation_id !== conversationId) return;
      const key = String(turnCreditKey);
      if (String(payload.turn_id) !== key && String(payload.usage.turnId) !== key) return;
      setLiveTurnCredits(payload.usage);
    };
    emitter.on('nomi.turn_credits.updated', handler);
    return () => {
      emitter.off('nomi.turn_credits.updated', handler);
    };
  }, [conversationId, turnCreditKey]);
  const turnCredits = liveTurnCredits ?? persistedTurnCredits;
  // Historical sessions (including eval shells) may lack live emit — backfill credits.
  useEffect(() => {
    if (isUserMessage || !conversationId || !turnCreditKey) return;
    if (conversation?.type !== 'nomi') return;
    if (turnCredits != null && (turnCredits.callCount > 0 || turnCredits.creditsConsumed > 0)) {
      return;
    }
    void fetchAndPersistTurnCredits({
      conversation_id: conversationId,
      turn_id: turnCreditKey,
      delayMs: 400,
    });
  }, [conversation?.type, conversationId, isUserMessage, turnCreditKey, turnCredits]);
  const { data: providerList } = useProvidersQuery({
    enabled: conversation?.type === 'nomi',
  });
  const creditProvider = useMemo(() => {
    if (conversation?.type !== 'nomi') return undefined;
    return findProviderById(providerList ?? [], conversation.model?.id) ?? conversation.model;
  }, [conversation, providerList]);
  const creditFallbackModel = conversation?.type === 'nomi' ? conversation.model?.use_model : undefined;
  const turnCreditDetails = useMemo(
    () => formatTurnCreditDetails(turnCredits?.calls, creditProvider),
    [creditProvider, turnCredits?.calls]
  );
  const turnCreditModels = useMemo(
    () => formatTurnCreditModels(turnCredits?.calls, creditFallbackModel, creditProvider),
    [creditFallbackModel, creditProvider, turnCredits?.calls]
  );
  // Show whenever the server recorded at least one billed call, or a positive
  // aggregate (covers laggy callCount while creditsConsumed already landed).
  const showTurnCredits =
    !isUserMessage &&
    shouldShowActions &&
    turnCredits != null &&
    (turnCredits.callCount > 0 || turnCredits.creditsConsumed > 0);
  // The list is virtualized, so off-screen rows unmount and remount on
  // scroll-back. Gating on arrival time keeps the entry animation to genuinely
  // new messages instead of replaying the whole history on every scroll.
  const enterAnimationRef = useRef<boolean | null>(null);
  if (enterAnimationRef.current === null) {
    enterAnimationRef.current =
      message.created_at != null && Date.now() - message.created_at < BUBBLE_ENTER_FRESH_MS;
  }
  const shouldPlayEnterAnimation = enterAnimationRef.current;
  const resolvedFiles = useMemo(
    () => files.map((file_path) => resolveMessageFilePath(file_path, conversationContext?.workspace)),
    [conversationContext?.workspace, files]
  );

  // 仅 Nomi、且为最近一条用户文本消息时可编辑（与后端"仅最近一条"对齐）。
  const messageList = useMessageList();
  const editableMessageId = message.message_id ?? message.msg_id;
  // C3: 编辑中徽章——SendBox 在编辑 store 登记的 msgId 与本气泡 durable id 一致。
  // C3: editing badge — SendBox registered this message's durable id in the
  // editing store, so this bubble shows the badge (+ pending = resubmitting).
  const editingState = useEditingMessage(conversationId);
  const isEditingThis = editingState != null && editableMessageId != null && editingState.msgId === editableMessageId;
  const isLatestUserMessage = useMemo(() => {
    if (!isUserMessage || !editableMessageId) return false;
    const lastRight = [...messageList].reverse().find((m) => m.position === 'right' && m.type === 'text');
    return (lastRight?.message_id ?? lastRight?.msg_id) === editableMessageId;
  }, [editableMessageId, isUserMessage, messageList]);

  const hasRenderableContent = contentToRender.trim().length > 0;

  // actionsOnly rows still need the actions chrome even if the mirrored
  // assistant text was filtered to empty (rare but legal).
  if (!actionsOnly && !hasRenderableContent && !writebackState) {
    return null;
  }

  const handleCopy = () => {
    const baseText = shouldRenderPlainText ? text : json ? JSON.stringify(data, null, 2) : text;
    const fileList = files.length ? `Files:\n${files.map((path) => `- ${path}`).join('\n')}\n\n` : '';
    const textToCopy = fileList + baseText;
    copyText(textToCopy)
      .then(() => {
        setShowCopyAlert(true);
        setTimeout(() => setShowCopyAlert(false), 2000);
        if (!isUserMessage) {
          confirmFirstValue({ source: 'copy_answer' });
          markFirstWinCompleted();
        }
      })
      .catch(() => {
        Message.error(t('common.copyFailed'));
      });
  };

  const copyButton = (
    <Tooltip content={t('common.copy', { defaultValue: 'Copy' })}>
      <button
        type='button'
        data-testid='message-copy-action'
        className='flex h-24px w-24px shrink-0 items-center justify-center rd-6px cursor-pointer text-t-secondary hover:bg-3 border-0 bg-transparent'
        onClick={handleCopy}
        style={{ lineHeight: 0 }}
        aria-label={t('common.copy', { defaultValue: 'Copy' })}
      >
        <Copy theme='outline' size='16' fill='currentColor' />
      </button>
    </Tooltip>
  );

  // 编辑（仅 Nomi 原生、且为最近一条用户文本消息）：把原文回填输入框并截断本地后续消息。
  const canEdit =
    conversationContext?.type === 'nomi' &&
    conversationContext.readOnly !== true &&
    conversationContext.isProcessing !== true &&
    isUserMessage &&
    message.type === 'text' &&
    editableMessageId != null &&
    message.created_at != null &&
    isLatestUserMessage;

  const isCodingProfile =
    typeof conversation?.extra?.task_profile === 'string' &&
    conversation.extra.task_profile.toLowerCase() === 'coding';

  const { data: codingRollbackAvailability } = useSWR(
    canEdit && isCodingProfile && conversationId && editableMessageId
      ? `coding-rollback/${conversationId}/${editableMessageId}`
      : null,
    () =>
      ipcBridge.conversation.codingTurnRollbackAvailability.invoke({
        conversation_id: conversationId!,
        msg_id: editableMessageId!,
      })
  );
  const canCodingRollback = canEdit && isCodingProfile && codingRollbackAvailability?.can_rollback === true;

  const handleEdit = () => {
    if (!editableMessageId || message.created_at == null) return;
    const rawContent = typeof message.content?.content === 'string' ? message.content.content : '';
    const { text: editText } = parseMessageFileMarker(rawContent, 'right');
    emitter.emit('sendbox.edit', { msgId: editableMessageId, createdAt: message.created_at, content: editText });
  };

  const handleCodingRollback = () => {
    if (!conversationId || !editableMessageId || !canCodingRollback) return;
    Modal.confirm({
      title: t('conversation.codingRollback.confirmTitle', { defaultValue: 'Roll back this turn?' }),
      content: t('conversation.codingRollback.confirmContent', {
        defaultValue:
          'Workspace files will be restored to before this turn, and this turn plus later messages will be removed. Nothing is resent automatically.',
      }),
      okText: t('conversation.codingRollback.confirmOk', { defaultValue: 'Roll back' }),
      okButtonProps: { status: 'danger' },
      onOk: async () => {
        try {
          await ipcBridge.conversation.codingTurnRollback.invoke({
            conversation_id: conversationId,
            msg_id: editableMessageId,
          });
          emitter.emit('conversation.messages.refresh', {
            conversationId,
            reason: 'coding-rollback',
          });
          emitter.emit('nomi.workspace.refresh');
          Message.success(t('conversation.codingRollback.success', { defaultValue: 'Turn rolled back' }));
        } catch (error) {
          const detail =
            error instanceof Error && error.message.trim().length > 0
              ? error.message
              : t('conversation.codingRollback.failed', {
                  defaultValue: "Couldn't roll back this turn. Please try again.",
                });
          Message.error(detail);
          throw new Error('coding-rollback-failed');
        }
      },
    });
  };

  const editButton = canEdit ? (
    <Tooltip content={t('conversation.editMessage.action', { defaultValue: 'Edit' })}>
      <button
        type='button'
        className='p-4px rd-6px cursor-pointer hover:bg-3 border-0 bg-transparent'
        onClick={handleEdit}
        style={{ lineHeight: 0 }}
        aria-label={t('conversation.editMessage.action', { defaultValue: 'Edit' })}
      >
        <Edit theme='outline' size='16' fill={iconColors.secondary} />
      </button>
    </Tooltip>
  ) : null;

  const codingRollbackButton = canCodingRollback ? (
    <Tooltip content={t('conversation.codingRollback.action', { defaultValue: 'Roll back turn' })}>
      <button
        type='button'
        data-testid='message-coding-rollback-action'
        className='p-4px rd-6px cursor-pointer hover:bg-3 border-0 bg-transparent'
        onClick={handleCodingRollback}
        style={{ lineHeight: 0 }}
        aria-label={t('conversation.codingRollback.action', { defaultValue: 'Roll back turn' })}
      >
        <Undo theme='outline' size='16' fill={iconColors.secondary} />
      </button>
    </Tooltip>
  ) : null;

  const cronMeta = message.content.cronMeta;
  const bubbleVariant = isUserMessage || cronMeta ? 'user' : isAgentMessage ? 'agent' : null;
  const senderName = message.content.senderName;
  const senderAgentType = message.content.senderAgentType;
  const senderConversationId = message.content.senderConversationId;
  const fallbackBackendLogo = senderAgentType ? getAgentLogo(senderAgentType) : null;
  const actionsRow = shouldShowActions ? (
    <div
      data-testid='message-actions'
      className={classNames('message-text-actions h-28px flex items-center mt-10px gap-6px text-t-secondary', {
        'flex-row-reverse': isUserMessage,
      })}
    >
      {copyButton}
      {editButton}
      {codingRollbackButton}
      {message.created_at && (
        <time
          className='message-text-actions__time text-12px leading-20px text-inherit select-none'
          dateTime={formatMessageTimeIso(message.created_at)}
        >
          {formatMessageTime(message.created_at)}
        </time>
      )}
      {showTurnCredits && turnCredits ? (
        <Tooltip
          content={turnCreditDetails}
          disabled={!turnCreditDetails}
        >
          <span
            data-testid='turn-credits'
            className='message-text-actions__credits inline-flex items-center gap-3px text-12px leading-20px text-inherit select-none tabular-nums'
          >
            {turnCreditModels
              ? t('messages.turnCredits.consumedBy', {
                  credits: turnCredits.creditsConsumed,
                  model: turnCreditModels,
                  defaultValue: '{{model}}消耗{{credits}}积分',
                })
              : t('messages.turnCredits.consumed', {
                  credits: turnCredits.creditsConsumed,
                  defaultValue: '消耗{{credits}}积分',
                })}
          </span>
        </Tooltip>
      ) : null}
    </div>
  ) : null;
  const copyAlert = showCopyAlert ? (
    <Alert
      type='success'
      content={t('messages.copySuccess')}
      showIcon
      className='message-copy-toast fixed top-20px left-50% transform -translate-x-50% z-9999 w-max max-w-[80%]'
      style={{ boxShadow: '0px 2px 12px rgba(0,0,0,0.12)' }}
      closable={false}
    />
  ) : null;

  if (actionsOnly) {
    return (
      <>
        {actionsRow}
        {copyAlert}
      </>
    );
  }

  return (
    <>
      <div className={classNames('min-w-0 flex w-full flex-col', isUserMessage ? 'items-end' : 'items-start')}>
        {cronMeta && <MessageCronBadge meta={cronMeta} />}
        {isAgentMessage && senderName && (
          <div className='flex items-center gap-6px mb-4px'>
            <AgentMessageAvatar
              senderName={senderName}
              senderConversationId={senderConversationId}
              backendLogo={fallbackBackendLogo}
            />
            <span className='text-12px text-t-secondary'>{senderName}</span>
          </div>
        )}
        {files.length > 0 && (
          <div className={classNames('mt-6px', { 'self-end': isUserMessage })}>
            {resolvedFiles.length === 1 ? (
              <div className='flex items-center'>
                <FilePreview path={resolvedFiles[0]} onRemove={() => undefined} readonly />
              </div>
            ) : (
              <HorizontalFileList>
                {resolvedFiles.map((path) => (
                  <FilePreview key={path} path={path} onRemove={() => undefined} readonly />
                ))}
              </HorizontalFileList>
            )}
          </div>
        )}
        {isEditingThis && (
          <div
            className={classNames(
              'mt-2px mb-2px flex items-center gap-4px text-12px text-t-secondary select-none',
              { 'self-end': isUserMessage }
            )}
          >
            {editingState?.pending ? (
              <Loading theme='outline' size='13' className='animate-spin' />
            ) : (
              <Edit theme='outline' size='13' />
            )}
            <span>
              {editingState?.phase === 'confirming'
                ? t('conversation.editMessage.confirmingBadge')
                : t('conversation.editMessage.editingBadge')}
            </span>
            {editingState?.phase === 'confirming' && editingState.continueConfirmation && (
              <Button
                type='text'
                size='mini'
                className='!h-20px !px-4px'
                onClick={(event) => {
                  event.stopPropagation();
                  editingState.continueConfirmation?.();
                }}
              >
                {t('conversation.editMessage.continueConfirmation')}
              </Button>
            )}
          </div>
        )}
        {hasRenderableContent && (
          <div
            className={classNames(
              'min-w-0 [&>p:first-child]:mt-0px [&>p:last-child]:mb-0px md:max-w-780px',
              bubbleVariant
                ? 'message-bubble px-10px py-7px md:px-12px md:py-9px'
                : 'w-full',
              {
                'message-bubble--user bg-aou-2': bubbleVariant === 'user',
                'message-bubble--agent bg-3': bubbleVariant === 'agent',
                'message-bubble-enter': shouldPlayEnterAnimation && !isStreaming,
                // C3: 编辑中气泡轻微置灰（正在回填/重发，行即将被替换）。
                // C3: slightly dim the bubble while its message is being edited
                // (recalled into the composer, or about to be replaced by resubmit).
                'opacity-70': isEditingThis,
              }
            )}
            data-testid='message-text-content'
          >
            <StreamingText status={message.status === 'finish' || !isStreaming ? 'done' : 'streaming'}>
              {/* JSON 内容使用折叠组件 Use CollapsibleContent for JSON content */}
              {shouldRenderPlainText ? (
                <div className={MESSAGE_BODY_CLASS_NAME}>
                  {text}
                </div>
              ) : json ? (
                <CollapsibleContent maxHeight={200} defaultCollapsed={true}>
                  <MarkdownView
                    codeStyle={CODE_STYLE}
                    fontSize={MESSAGE_BODY_FONT_SIZE}
                    lineHeight={MESSAGE_BODY_LINE_HEIGHT}
                    allowUnverifiedImages={isUserMessage}
                  >{`\`\`\`json\n${JSON.stringify(data, null, 2)}\n\`\`\``}</MarkdownView>
                </CollapsibleContent>
              ) : streamingParts && streamingParts.tailKind === 'code' ? (
                <>
                  {streamingParts.stablePrefix ? (
                    <MarkdownView
                      codeStyle={CODE_STYLE}
                      fontSize={MESSAGE_BODY_FONT_SIZE}
                      lineHeight={MESSAGE_BODY_LINE_HEIGHT}
                      allowUnverifiedImages={isUserMessage}
                      isStreaming
                    >
                      {streamingParts.stablePrefix}
                    </MarkdownView>
                  ) : null}
                  <CodeBlock language={streamingParts.codeLanguage} streaming>
                    {streamingParts.codeContent ?? ''}
                  </CodeBlock>
                </>
              ) : streamingParts ? (
                <>
                  {streamingParts.stablePrefix ? (
                    <MarkdownView
                      codeStyle={CODE_STYLE}
                      fontSize={MESSAGE_BODY_FONT_SIZE}
                      lineHeight={MESSAGE_BODY_LINE_HEIGHT}
                      allowUnverifiedImages={isUserMessage}
                      isStreaming={isStreaming}
                    >
                      {streamingParts.stablePrefix}
                    </MarkdownView>
                  ) : null}
                  {streamingParts.tail ? (
                    <div className={`${MESSAGE_BODY_CLASS_NAME} message-streaming-body`}>
                      {streamingParts.tail}
                    </div>
                  ) : null}
                </>
              ) : (
                <MarkdownView
                  codeStyle={CODE_STYLE}
                  fontSize={MESSAGE_BODY_FONT_SIZE}
                  lineHeight={MESSAGE_BODY_LINE_HEIGHT}
                  allowUnverifiedImages={isUserMessage}
                >
                  {data}
                </MarkdownView>
              )}
            </StreamingText>
          </div>
        )}
        {writebackState && (
          <MessageKnowledgeWriteback
            state={writebackState}
            conversationId={message.conversation_id}
            messageId={message.message_id ?? message.msg_id}
            onSettled={
              conversationId && turnCreditKey
                ? () => {
                    void fetchAndPersistTurnCredits({
                      conversation_id: conversationId,
                      turn_id: turnCreditKey,
                      force: true,
                      delayMs: 800,
                    });
                  }
                : undefined
            }
          />
        )}
        {actionsRow}
      </div>
      {copyAlert}
    </>
  );
};

export default MessageText;
