/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { Popover } from '@arco-design/web-react';
import { Caution, Close, Info, Plus } from '@icon-park/react';
import { useTranslation } from 'react-i18next';
import NomiModal from '@/renderer/components/base/NomiModal';
import ErrorDiagnosticContent from '@/renderer/components/base/ErrorDiagnosticContent';
import { buildAgentErrorDiagnostic } from '@/renderer/utils/ui/errorDiagnostics';
import {
  getConversationErrorReportContextKey,
  MAX_CONVERSATION_ERROR_REPORT_DESCRIPTION_CHARS,
  type ConversationErrorReportContext,
  type ConversationErrorReportDraft,
  type ConversationErrorReportSubmitResult,
} from '../conversationErrorReport';
import SupportImagePreviewGrid from './SupportImagePreviewGrid';
import { supportImagePreviewCache } from '../state/supportImagePreviewCache';
import {
  MAX_SUPPORT_IMAGES,
  revokeSupportImagePreview,
  SUPPORT_IMAGE_ACCEPT,
  selectSupportImagePreviews,
  type SupportImagePreviewItem,
} from '../supportImageAttachments';

type ConversationErrorReportModalProps = {
  context: ConversationErrorReportContext | null;
  onCancel: () => void;
  onSubmit: (draft: ConversationErrorReportDraft) => Promise<ConversationErrorReportSubmitResult>;
  onOpenSupportChat: () => void;
};

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
  const [infoOpen, setInfoOpen] = useState(false);
  const [submitStatus, setSubmitStatus] = useState<
    'idle' | 'preparation-failed' | 'partial-failure' | 'invalid-input'
  >('idle');
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const screenshotsRef = useRef(screenshots);
  const providerOwnedPreviewUrlsRef = useRef(new Set<string>());
  const lastContextKeyRef = useRef<string | null>(null);
  const charCount = Array.from(description).length;
  const descriptionTooLong = charCount > MAX_CONVERSATION_ERROR_REPORT_DESCRIPTION_CHARS;
  const diagnostic = context ? buildAgentErrorDiagnostic(context.error) : null;

  useEffect(() => {
    screenshotsRef.current = screenshots;
  }, [screenshots]);

  useEffect(() => {
    return () => {
      for (const screenshot of screenshotsRef.current) {
        if (
          !providerOwnedPreviewUrlsRef.current.has(screenshot.url) &&
          !supportImagePreviewCache.hasPreviewUrl(screenshot.url)
        ) {
          revokeSupportImagePreview(screenshot);
        }
      }
    };
  }, []);

  const revokeDraftScreenshots = useCallback((items: SupportImagePreviewItem[]) => {
    for (const screenshot of items) {
      if (
        !providerOwnedPreviewUrlsRef.current.has(screenshot.url) &&
        !supportImagePreviewCache.hasPreviewUrl(screenshot.url)
      ) {
        revokeSupportImagePreview(screenshot);
      }
    }
  }, []);

  useEffect(() => {
    if (!context) return;
    const nextKey = getConversationErrorReportContextKey(context);
    if (lastContextKeyRef.current !== nextKey) {
      revokeDraftScreenshots(screenshotsRef.current);
      screenshotsRef.current = [];
      setDescription('');
      setScreenshots([]);
      setSubmitStatus('idle');
      setInfoOpen(false);
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
    const current = screenshotsRef.current;
    const target = current.find((item) => item.id === id);
    if (
      target &&
      !providerOwnedPreviewUrlsRef.current.has(target.url) &&
      !supportImagePreviewCache.hasPreviewUrl(target.url)
    ) {
      revokeSupportImagePreview(target);
    }
    const next = current.filter((item) => item.id !== id);
    screenshotsRef.current = next;
    setScreenshots(next);
  };

  const handleScreenshotSelected = (event: React.ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(event.target.files ?? []);
    event.target.value = '';
    if (files.length === 0) return;

    const remaining = MAX_SUPPORT_IMAGES - screenshotsRef.current.length;
    if (remaining <= 0) {
      Message.warning(
        t('settings.bugReportScreenshotLimit', {
          defaultValue: '最多上传 {{count}} 张截图',
          count: MAX_SUPPORT_IMAGES,
        })
      );
      return;
    }

    const { items: accepted, rejected, truncated } = selectSupportImagePreviews(files, remaining);

    if (accepted.length > 0) {
      const current = screenshotsRef.current;
      const next = [...current, ...accepted];
      screenshotsRef.current = next;
      setScreenshots(next);
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
    if (preserveScreenshotPreviews) {
      for (const screenshot of screenshotsRef.current) {
        providerOwnedPreviewUrlsRef.current.delete(screenshot.url);
      }
    }
    screenshotsRef.current = [];
    setDescription('');
    setScreenshots([]);
    setSubmitStatus('idle');
  };

  const handleSubmit = async () => {
    if (!context || submitting || descriptionTooLong || submitStatus === 'partial-failure') return;
    setInfoOpen(false);
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
      } else if (result.status === 'invalid-input') {
        setSubmitStatus('invalid-input');
        Message.error(
          t('settings.bugReportInvalidInput', {
            defaultValue: '反馈内容或截图不符合要求，请检查后重试。',
          })
        );
      } else if (result.status === 'stale') {
        // The authenticated account or report context changed while the
        // request was in flight. The accepted message belongs to the old
        // context and must not update this modal.
        return;
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
    setInfoOpen(false);
    onCancel();
  };

  const handleOpenSupportChat = () => {
    if (submitting) return;
    setInfoOpen(false);
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
      alignCenter
      wrapClassName='conversation-error-report__wrapper'
      style={{
        width: 'min(720px, calc(100vw - 32px))',
        maxWidth: 'min(720px, calc(100vw - 32px))',
        maxHeight: 'min(760px, calc(100dvh - 32px))',
      }}
      contentStyle={{ padding: 0, overflow: 'hidden' }}
      maskClosable={false}
      autoFocus={false}
    >
      {context && diagnostic ? (
        <div className='conversation-error-report__body'>
          <div className='conversation-error-report__scroll'>
            <section
              className='conversation-error-report__editor'
              aria-labelledby='conversation-error-report-editor-title'
            >
              <h3 id='conversation-error-report-editor-title' className='sr-only'>
                {t('settings.bugReportDescriptionLabel')}
              </h3>
              <textarea
                id='conversation-error-report-description'
                ref={textareaRef}
                className='conversation-error-report__textarea'
                value={description}
                maxLength={MAX_CONVERSATION_ERROR_REPORT_DESCRIPTION_CHARS}
                placeholder={t('settings.bugReportDescriptionPlaceholder')}
                aria-invalid={descriptionTooLong}
                aria-labelledby='conversation-error-report-editor-title'
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
              {screenshots.length > 0 ? (
                <SupportImagePreviewGrid
                  items={screenshots}
                  onRemove={removeScreenshot}
                  disabled={submitting}
                  className='conversation-error-report__screenshots'
                />
              ) : null}
              <div className='conversation-error-report__editor-toolbar'>
                <div className='conversation-error-report__editor-actions'>
                  {screenshots.length < MAX_SUPPORT_IMAGES ? (
                    <>
                      <input
                        ref={fileInputRef}
                        type='file'
                        accept={SUPPORT_IMAGE_ACCEPT}
                        multiple
                        className='hidden'
                        onChange={handleScreenshotSelected}
                        disabled={submitting}
                      />
                      <button
                        type='button'
                        className='conversation-error-report__attachment'
                        onClick={() => fileInputRef.current?.click()}
                        disabled={submitting}
                        aria-label={t('settings.bugReportScreenshotLabel')}
                        title={t('settings.bugReportScreenshotFormats', {
                          defaultValue: '支持 PNG / JPG，最多 4 张，单张不超过 5MB',
                        })}
                      >
                        <Plus theme='outline' size='16' strokeWidth={2} />
                      </button>
                    </>
                  ) : null}
                  {screenshots.length > 0 ? (
                    <span className='conversation-error-report__attachment-count' aria-live='polite'>
                      {screenshots.length} / {MAX_SUPPORT_IMAGES}
                    </span>
                  ) : null}
                </div>
                <div className='conversation-error-report__editor-meta'>
                  <div
                    id='conversation-error-report-description-count'
                    className={
                      descriptionTooLong
                        ? 'conversation-error-report__count conversation-error-report__count--danger'
                        : 'conversation-error-report__count'
                    }
                  >
                    {charCount} / {MAX_CONVERSATION_ERROR_REPORT_DESCRIPTION_CHARS}
                  </div>
                  <Popover
                    className='conversation-error-report__info-popover'
                    trigger={['hover', 'click', 'focus']}
                    position='top'
                    popupVisible={infoOpen}
                    onVisibleChange={setInfoOpen}
                    getPopupContainer={() => document.body}
                    content={
                      <div
                        id='conversation-error-report-auto-info'
                        className='conversation-error-report__info-content'
                        role='dialog'
                        aria-label={t('settings.bugReportIncludedInfo', {
                          defaultValue: '查看自动附带信息说明',
                        })}
                      >
                        <p>{t('settings.bugReportAutoInfo')}</p>
                        <p>{t('settings.bugReportLogPrivacy')}</p>
                      </div>
                    }
                  >
                    <button
                      type='button'
                      className='conversation-error-report__info-trigger'
                      aria-label={t('settings.bugReportIncludedInfo', {
                        defaultValue: '查看自动附带信息说明',
                      })}
                      aria-haspopup='dialog'
                      aria-controls='conversation-error-report-auto-info'
                      aria-expanded={infoOpen}
                    >
                      <Info theme='outline' size='15' strokeWidth={2} />
                    </button>
                  </Popover>
                </div>
              </div>
            </section>

            <section
              className='conversation-error-report__diagnostic-section'
              aria-labelledby='conversation-error-report-diagnostic-title'
            >
              <h3
                id='conversation-error-report-diagnostic-title'
                className='conversation-error-report__diagnostic-heading'
              >
                {t('settings.bugReportDiagnostic', { defaultValue: '错误诊断' })}
              </h3>
              <ErrorDiagnosticContent
                diagnostic={diagnostic}
                className='conversation-error-report__diagnostic'
              />
            </section>

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
