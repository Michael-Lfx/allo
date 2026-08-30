/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { Caution, Close } from '@icon-park/react';
import { useTranslation } from 'react-i18next';
import NomiModal from '@/renderer/components/base/NomiModal';
import ErrorDiagnosticContent from '@/renderer/components/base/ErrorDiagnosticContent';
import { buildAgentErrorDiagnostic } from '@/renderer/utils/ui/errorDiagnostics';
import type {
  ConversationErrorReportContext,
  ConversationErrorReportDraft,
  ConversationErrorReportSubmitResult,
} from '../conversationErrorReport';
import SupportImagePreviewGrid from './SupportImagePreviewGrid';
import {
  createSupportImagePreviewId,
  isAcceptedSupportImage,
  MAX_SUPPORT_IMAGE_BYTES,
  MAX_SUPPORT_IMAGES,
  revokeSupportImagePreview,
  SUPPORT_IMAGE_ACCEPT,
  type SupportImagePreviewItem,
} from '../supportImageAttachments';

const MAX_REPORT_DESCRIPTION_CHARS = 4000;

type ConversationErrorReportModalProps = {
  context: ConversationErrorReportContext | null;
  onCancel: () => void;
  onSubmit: (draft: ConversationErrorReportDraft) => Promise<ConversationErrorReportSubmitResult>;
  onOpenSupportChat: () => void;
};

function contextKey(context: ConversationErrorReportContext): string {
  return [
    context.conversationId,
    context.messageId ?? '',
    context.turnId ?? '',
    context.occurredAt,
  ].join(':');
}

const ConversationErrorReportModal: React.FC<ConversationErrorReportModalProps> = ({
  context,
  onCancel,
  onSubmit,
  onOpenSupportChat,
}) => {
  const { t } = useTranslation();
  const [description, setDescription] = useState('');
  const [screenshots, setScreenshots] = useState<SupportImagePreviewItem[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [submitStatus, setSubmitStatus] = useState<
    'idle' | 'preparation-failed' | 'partial-failure'
  >('idle');
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const screenshotsRef = useRef(screenshots);
  const providerOwnedPreviewUrlsRef = useRef(new Set<string>());
  const lastContextKeyRef = useRef<string | null>(null);
  const charCount = Array.from(description).length;
  const descriptionTooLong = charCount > MAX_REPORT_DESCRIPTION_CHARS;
  const diagnostic = context ? buildAgentErrorDiagnostic(context.error) : null;

  useEffect(() => {
    screenshotsRef.current = screenshots;
  }, [screenshots]);

  useEffect(() => {
    return () => {
      for (const screenshot of screenshotsRef.current) {
        if (!providerOwnedPreviewUrlsRef.current.has(screenshot.url)) {
          revokeSupportImagePreview(screenshot);
        }
      }
    };
  }, []);

  const revokeDraftScreenshots = useCallback((items: SupportImagePreviewItem[]) => {
    for (const screenshot of items) {
      if (!providerOwnedPreviewUrlsRef.current.has(screenshot.url)) {
        revokeSupportImagePreview(screenshot);
      }
    }
  }, []);

  useEffect(() => {
    if (!context) return;
    const nextKey = contextKey(context);
    if (lastContextKeyRef.current !== nextKey) {
      revokeDraftScreenshots(screenshotsRef.current);
      setDescription('');
      setScreenshots([]);
      setSubmitStatus('idle');
      lastContextKeyRef.current = nextKey;
    }

    const focus = () => textareaRef.current?.focus();
    if (typeof window !== 'undefined') {
      window.requestAnimationFrame(focus);
    } else {
      focus();
    }
  }, [context, revokeDraftScreenshots]);

  const removeScreenshot = (id: string) => {
    setScreenshots((current) => {
      const target = current.find((item) => item.id === id);
      if (target && !providerOwnedPreviewUrlsRef.current.has(target.url)) {
        revokeSupportImagePreview(target);
      }
      return current.filter((item) => item.id !== id);
    });
  };

  const handleScreenshotSelected = (event: React.ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(event.target.files ?? []);
    event.target.value = '';
    if (files.length === 0) return;

    const remaining = MAX_SUPPORT_IMAGES - screenshots.length;
    if (remaining <= 0) {
      Message.warning(
        t('settings.bugReportScreenshotLimit', {
          defaultValue: '最多上传 {{count}} 张截图',
          count: MAX_SUPPORT_IMAGES,
        })
      );
      return;
    }

    const accepted: SupportImagePreviewItem[] = [];
    let rejected = false;
    let truncated = false;
    for (const file of files) {
      if (accepted.length >= remaining) {
        truncated = true;
        break;
      }
      if (!isAcceptedSupportImage(file) || file.size > MAX_SUPPORT_IMAGE_BYTES) {
        rejected = true;
        continue;
      }
      accepted.push({
        id: createSupportImagePreviewId(file),
        url: URL.createObjectURL(file),
        file,
      });
    }

    if (accepted.length > 0) {
      setScreenshots((current) => [...current, ...accepted].slice(0, MAX_SUPPORT_IMAGES));
    }
    if (rejected) {
      Message.warning(
        t('settings.bugReportScreenshotFormats', {
          defaultValue: '仅支持 PNG / JPG，且单张不超过 5MB',
        })
      );
    } else if (truncated) {
      Message.warning(
        t('settings.bugReportScreenshotLimit', {
          defaultValue: '最多上传 {{count}} 张截图',
          count: MAX_SUPPORT_IMAGES,
        })
      );
    }
  };

  const clearDraft = ({ preserveScreenshotPreviews = false } = {}) => {
    // Once the provider has queued a report image, its object URL is owned by
    // the pending support message/cache so the image does not disappear when
    // this modal closes. Draft-only images still belong to the modal.
    if (!preserveScreenshotPreviews) {
      revokeDraftScreenshots(screenshotsRef.current);
    }
    setDescription('');
    setScreenshots([]);
    setSubmitStatus('idle');
  };

  const handleSubmit = async () => {
    if (!context || submitting || descriptionTooLong || submitStatus === 'partial-failure') return;
    setSubmitting(true);
    setSubmitStatus('idle');
    try {
      const result = await onSubmit({
        description,
        screenshots: screenshots.map((screenshot) => ({
          file: screenshot.file,
          fileName: screenshot.file.name,
          previewUrl: screenshot.url,
        })),
      });
      if (result.status === 'success') {
        Message.success(t('settings.bugReportSuccess'));
        screenshots.forEach((screenshot) => {
          providerOwnedPreviewUrlsRef.current.add(screenshot.url);
        });
        clearDraft({ preserveScreenshotPreviews: true });
        onCancel();
      } else if (result.status === 'partial-failure') {
        screenshots.forEach((screenshot) => {
          providerOwnedPreviewUrlsRef.current.add(screenshot.url);
        });
        setSubmitStatus('partial-failure');
        Message.error(t('settings.bugReportPartialFailure'));
      } else {
        setSubmitStatus('preparation-failed');
        Message.error(t('settings.bugReportError'));
      }
    } catch {
      setSubmitStatus('preparation-failed');
      Message.error(t('settings.bugReportError'));
    } finally {
      setSubmitting(false);
    }
  };

  const handleCancel = () => {
    if (submitting) return;
    onCancel();
  };

  const handleOpenSupportChat = () => {
    if (submitting) return;
    onCancel();
    onOpenSupportChat();
  };

  return (
    <NomiModal
      visible={context !== null}
      onCancel={handleCancel}
      header={{
        title: (
          <span className='conversation-error-report__title'>
            <span className='conversation-error-report__title-icon' aria-hidden>
              <Caution theme='outline' size='18' fill='currentColor' />
            </span>
            <span>{t('settings.bugReportTitle')}</span>
          </span>
        ),
        showClose: true,
        closeIcon: <Close size={18} fill='currentColor' className='block' />,
        className: 'conversation-error-report__header',
      }}
      footer={
        <div className='conversation-error-report__footer-actions'>
          <button
            type='button'
            className='conversation-error-report__button conversation-error-report__button--secondary'
            onClick={handleCancel}
            disabled={submitting}
          >
            {t('settings.bugReportCancel')}
          </button>
          {submitStatus === 'partial-failure' ? (
            <button
              type='button'
              className='conversation-error-report__button conversation-error-report__button--primary'
              onClick={handleOpenSupportChat}
            >
              {t('settings.bugReportOpenSupport')}
            </button>
          ) : (
            <button
              type='button'
              className='conversation-error-report__button conversation-error-report__button--primary'
              onClick={() => void handleSubmit()}
              disabled={submitting || descriptionTooLong}
            >
              {submitting
                ? t('settings.bugReportSubmitting')
                : t('settings.bugReportSubmit')}
            </button>
          )}
        </div>
      }
      className='conversation-error-report-modal w-[min(720px,calc(100vw-32px))] max-w-720px rd-16px'
      style={{
        width: 'min(720px, calc(100vw - 32px))',
        maxWidth: 'min(720px, calc(100vw - 32px))',
        height: 'min(760px, calc(100dvh - 32px))',
        maxHeight: 'min(760px, calc(100dvh - 32px))',
      }}
      contentStyle={{ padding: 0, overflow: 'hidden' }}
      unmountOnExit
      autoFocus={false}
    >
      {context && diagnostic ? (
        <div className='conversation-error-report__body'>
          <div className='conversation-error-report__scroll'>
            <p className='conversation-error-report__intro'>
              {t('settings.bugReportIntro', {
                defaultValue: '请描述问题表现和复现步骤，帮助我们更快定位。',
              })}
            </p>

            <div className='conversation-error-report__section'>
              <label
                className='conversation-error-report__label'
                htmlFor='conversation-error-report-description'
              >
                {t('settings.bugReportDescriptionLabel')}
                <span className='conversation-error-report__optional'>
                  {t('settings.bugReportOptional', { defaultValue: '可选' })}
                </span>
              </label>
              <textarea
                id='conversation-error-report-description'
                ref={textareaRef}
                className='conversation-error-report__textarea'
                value={description}
                maxLength={MAX_REPORT_DESCRIPTION_CHARS}
                placeholder={t('settings.bugReportDescriptionPlaceholder')}
                aria-invalid={descriptionTooLong}
                aria-describedby='conversation-error-report-description-count'
                onChange={(event) => setDescription(event.target.value)}
                onKeyDown={(event) => {
                  if (
                    event.key === 'Enter' &&
                    (event.ctrlKey || event.metaKey) &&
                    !event.nativeEvent.isComposing
                  ) {
                    event.preventDefault();
                    void handleSubmit();
                  }
                }}
              />
              <div
                id='conversation-error-report-description-count'
                className={
                  descriptionTooLong
                    ? 'conversation-error-report__count conversation-error-report__count--danger'
                    : 'conversation-error-report__count'
                }
              >
                {charCount} / {MAX_REPORT_DESCRIPTION_CHARS}
              </div>
            </div>

            <div className='conversation-error-report__section'>
              <div className='conversation-error-report__section-heading'>
                <span className='conversation-error-report__label'>
                  {t('settings.bugReportScreenshotLabel')}
                  <span className='conversation-error-report__optional'>
                    {t('settings.bugReportOptional', { defaultValue: '可选' })}
                  </span>
                </span>
                <span className='conversation-error-report__hint'>
                  {t('settings.bugReportScreenshotFormats', {
                    defaultValue: 'PNG / JPG，单张不超过 5MB',
                  })}
                </span>
              </div>
              <SupportImagePreviewGrid
                items={screenshots}
                onRemove={removeScreenshot}
                disabled={submitting}
                className='conversation-error-report__screenshots'
              />
              {screenshots.length < MAX_SUPPORT_IMAGES ? (
                <label
                  className='conversation-error-report__upload'
                  role='button'
                  tabIndex={submitting ? -1 : 0}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter' || event.key === ' ') {
                      event.preventDefault();
                      fileInputRef.current?.click();
                    }
                  }}
                >
                  <input
                    ref={fileInputRef}
                    type='file'
                    accept={SUPPORT_IMAGE_ACCEPT}
                    multiple
                    className='hidden'
                    onChange={handleScreenshotSelected}
                    disabled={submitting}
                  />
                  <span>{t('settings.bugReportScreenshotDropzoneText')}</span>
                </label>
              ) : null}
            </div>

            <div className='conversation-error-report__section conversation-error-report__auto-info'>
              <div className='conversation-error-report__label'>
                {t('settings.bugReportIncludedInfo', { defaultValue: '自动附带信息' })}
              </div>
              <p className='conversation-error-report__copy'>
                {t('settings.bugReportAutoInfo')}
              </p>
              <p className='conversation-error-report__copy conversation-error-report__copy--muted'>
                {t('settings.bugReportLogPrivacy', {
                  defaultValue: '日志仅用于排查问题，可能包含对话记录和设备信息。',
                })}
              </p>
            </div>

            <div className='conversation-error-report__section'>
              <div className='conversation-error-report__label'>
                {t('settings.bugReportDiagnostic', { defaultValue: '错误诊断' })}
              </div>
              <ErrorDiagnosticContent
                diagnostic={diagnostic}
                className='conversation-error-report__diagnostic'
              />
            </div>

            {submitStatus === 'preparation-failed' ? (
              <div className='conversation-error-report__status' role='alert'>
                {t('settings.bugReportRetryHint', {
                  defaultValue: '日志或截图准备失败，草稿已保留，请重试。',
                })}
              </div>
            ) : null}
            {submitStatus === 'partial-failure' ? (
              <div className='conversation-error-report__status' role='alert'>
                {t('settings.bugReportPartialFailureHint', {
                  defaultValue: '部分内容已进入客服消息，打开客服窗口即可查看并重试失败项。',
                })}
              </div>
            ) : null}
          </div>
        </div>
      ) : null}
    </NomiModal>
  );
};

export default ConversationErrorReportModal;
