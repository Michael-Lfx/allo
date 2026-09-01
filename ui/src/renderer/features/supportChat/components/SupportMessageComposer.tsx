/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Trigger } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { FileText, ImageFiles, Plus } from '@icon-park/react';
import React, { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ICloudImAttachmentPayload } from '@/common/adapter/ipcBridge';
import { useCloudAuth } from '@/renderer/hooks/context/CloudAuthContext';
import { supportChatApi } from '../api/supportChatApi';
import { MAX_SUPPORT_MESSAGE_CHARS } from '../api/supportChatTypes';
import { collectSupportDeviceInfo } from '../collectSupportDeviceInfo';
import { collectSupportLogUserInfo } from '../collectSupportLogUserInfo';
import SupportImagePreviewGrid from './SupportImagePreviewGrid';
import {
  buildSupportLogPayload,
  MAX_SUPPORT_IMAGES,
  revokeSupportImagePreview,
  revokeSupportImagePreviews,
  SUPPORT_IMAGE_ACCEPT,
  selectSupportImagePreviews,
  SupportAttachmentReferenceError,
  type SupportImagePreviewItem,
} from '../supportImageAttachments';

const DEFAULT_LOG_CONTENT = '附上日志，请协助排查';

type SupportMessageComposerProps = {
  disabled?: boolean;
  onSend: (content: string, logPayload?: ICloudImAttachmentPayload) => Promise<boolean>;
  /** 同步挂 pending 气泡，上传/发送在 provider 后台完成。 */
  onSendImages: (params: {
    content: string;
    images: { file: File; fileName: string; previewUrl: string }[];
  }) => boolean;
};

const SupportMessageComposer: React.FC<SupportMessageComposerProps> = ({
  disabled,
  onSend,
  onSendImages,
}) => {
  const { t } = useTranslation();
  const { whoami } = useCloudAuth();
  const [value, setValue] = useState('');
  const [sending, setSending] = useState(false);
  const [attachmentMenuOpen, setAttachmentMenuOpen] = useState(false);
  const [imagePreviews, setImagePreviews] = useState<SupportImagePreviewItem[]>([]);
  const [preparingLogs, setPreparingLogs] = useState(false);
  const [logConfirmOpen, setLogConfirmOpen] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const imagePreviewsRef = useRef(imagePreviews);
  const charCount = Array.from(value).length;
  const canSend =
    !disabled &&
    !sending &&
    !preparingLogs &&
    (charCount > 0 || imagePreviews.length > 0) &&
    charCount <= MAX_SUPPORT_MESSAGE_CHARS;
  const canAddImages = imagePreviews.length < MAX_SUPPORT_IMAGES;

  useLayoutEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea) return;
    textarea.style.height = 'auto';
    textarea.style.height = `${Math.min(120, Math.max(60, textarea.scrollHeight))}px`;
  }, [value]);

  useEffect(() => {
    imagePreviewsRef.current = imagePreviews;
  }, [imagePreviews]);

  useEffect(() => {
    return () => {
      revokeSupportImagePreviews(imagePreviewsRef.current);
    };
  }, []);

  const removeImagePreview = (id: string) => {
    const current = imagePreviewsRef.current;
    const target = current.find((item) => item.id === id);
    if (target) revokeSupportImagePreview(target);
    const next = current.filter((item) => item.id !== id);
    imagePreviewsRef.current = next;
    setImagePreviews(next);
  };

  const handleSend = async () => {
    if (!canSend) return;
    const trimmed = value.trim();
    const images = imagePreviews;
    if (!trimmed && images.length === 0) return;

    if (images.length > 0) {
      // 秒上屏：预览所有权交给 pending 气泡，立即清空输入区；上传/发送由 provider 后台处理。
      const accepted = onSendImages({
        content: trimmed,
        images: images.map((item) => ({
          file: item.file,
          fileName: item.file.name,
          previewUrl: item.url,
        })),
      });
      if (!accepted) return;
      setValue('');
      setImagePreviews([]);
      imagePreviewsRef.current = [];
      if (fileInputRef.current) {
        fileInputRef.current.value = '';
      }
      return;
    }

    setSending(true);
    try {
      const sent = await onSend(trimmed);
      if (sent) setValue('');
    } catch {
      // pending failure is shown in the message list
    } finally {
      setSending(false);
    }
  };

  const prepareAndUploadLogs = async () => {
    setPreparingLogs(true);
    try {
      const packed = await supportChatApi.packLogs();
      const uploaded = await supportChatApi.uploadLogFromPath({
        zipPath: packed.zipPath,
        fileName: packed.fileName,
      });
      const account = collectSupportLogUserInfo(whoami);
      const device = await collectSupportDeviceInfo();
      const content = t('common.supportChat.uploadLogsDefaultContent', {
        defaultValue: DEFAULT_LOG_CONTENT,
      });
      const sent = await onSend(
        content,
        buildSupportLogPayload(
          uploaded,
          {
            fileName: packed.fileName,
            contentType: 'application/zip',
            byteSize: packed.byteSize,
          },
          { account, device }
        )
      );
      if (!sent) {
        throw new Error(
          t('common.supportChat.uploadLogsFailed', {
            defaultValue: '日志打包、上传或发送失败',
          })
        );
      }
      Message.success(
        t('common.supportChat.uploadLogsReady', {
          defaultValue: '日志已上传并发送',
        })
      );
      setLogConfirmOpen(false);
    } catch (error) {
      Message.error(
        error instanceof SupportAttachmentReferenceError
          ? t('common.supportChat.uploadLogsFailed', {
              defaultValue: '日志打包、上传或发送失败',
            })
          : error instanceof Error && error.message
            ? error.message
            : t('common.supportChat.uploadLogsFailed', {
                defaultValue: '日志打包、上传或发送失败',
              })
      );
    } finally {
      setPreparingLogs(false);
    }
  };

  const showUploadLogsConfirm = () => {
    setAttachmentMenuOpen(false);
    setLogConfirmOpen(true);
  };

  const openImagePicker = () => {
    setAttachmentMenuOpen(false);
    if (!canAddImages) {
      Message.warning(
        t('common.supportChat.imageLimitReached', {
          defaultValue: '最多上传 4 张图片',
          count: MAX_SUPPORT_IMAGES,
        })
      );
      return;
    }
    fileInputRef.current?.click();
  };

  const handleImageSelected = (event: React.ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(event.target.files ?? []);
    event.target.value = '';
    if (files.length === 0) return;

    const remaining = MAX_SUPPORT_IMAGES - imagePreviewsRef.current.length;
    if (remaining <= 0) {
      Message.warning(
        t('common.supportChat.imageLimitReached', {
          defaultValue: '最多上传 4 张图片',
          count: MAX_SUPPORT_IMAGES,
        })
      );
      return;
    }

    const { items: accepted, rejected, truncated } = selectSupportImagePreviews(files, remaining);

    if (accepted.length > 0) {
      const current = imagePreviewsRef.current;
      const next = [...current, ...accepted];
      imagePreviewsRef.current = next;
      setImagePreviews(next);
    }

    if (rejected) {
      Message.warning(
        t('common.supportChat.invalidImage', {
          defaultValue: '仅支持 PNG / JPG，且单张小于 5MB',
        })
      );
    } else if (truncated) {
      Message.warning(
        t('common.supportChat.imageLimitReached', {
          defaultValue: '最多上传 4 张图片',
          count: MAX_SUPPORT_IMAGES,
        })
      );
    }
  };

  const attachmentMenu = (
    <div className='support-chat-composer__attachment-menu w-180px p-4px rd-10px border border-solid border-[var(--color-border-2)] bg-[var(--dialog-fill-0)] shadow-lg'>
      <button
        type='button'
        className='w-full h-32px px-8px flex items-center gap-8px rd-7px border-none bg-transparent text-13px leading-18px text-t-primary cursor-pointer transition-colors hover:bg-fill-2'
        onClick={openImagePicker}
      >
        <span className='support-chat-composer__icon'>
          <ImageFiles theme='outline' size='16' fill='currentColor' />
        </span>
        <span>{t('common.supportChat.uploadImage', { defaultValue: '上传图片' })}</span>
      </button>
      <button
        type='button'
        className='w-full h-32px px-8px flex items-center gap-8px rd-7px border-none bg-transparent text-13px leading-18px text-t-primary cursor-pointer transition-colors hover:bg-fill-2'
        onClick={showUploadLogsConfirm}
      >
        <span className='support-chat-composer__icon'>
          <FileText theme='outline' size='16' fill='currentColor' />
        </span>
        <span>{t('common.supportChat.uploadLogs', { defaultValue: '上传日志' })}</span>
      </button>
    </div>
  );

  return (
    <div className='support-chat-composer border-t border-[var(--color-border-2)] flex shrink-0 flex-col gap-6px'>
      <input
        ref={fileInputRef}
        type='file'
        accept={SUPPORT_IMAGE_ACCEPT}
        multiple
        className='hidden'
        onChange={handleImageSelected}
      />

      <div className='support-chat-composer__surface box-border w-full min-h-104px px-6px pt-6px pb-4px flex flex-col gap-0 rd-20px border border-solid border-[var(--color-border-2)] bg-bg-1 transition-colors'>
        <SupportImagePreviewGrid
          items={imagePreviews}
          onRemove={removeImagePreview}
          disabled={disabled || sending || preparingLogs}
          className='mb-6px'
        />

        <textarea
          ref={textareaRef}
          className='box-border w-full min-h-60px max-h-120px overflow-y-auto resize-none border-none bg-transparent px-2px py-2px text-14px leading-22px text-t-primary outline-none disabled:opacity-60 disabled:cursor-not-allowed'
          rows={2}
          value={value}
          maxLength={MAX_SUPPORT_MESSAGE_CHARS}
          disabled={disabled || sending || preparingLogs}
          aria-label={t('common.supportChat.composerPlaceholder', {
            defaultValue: '描述你的问题…',
          })}
          placeholder={t('common.supportChat.composerPlaceholder', {
            defaultValue: '描述你的问题…',
          })}
          onChange={(event) => setValue(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
              event.preventDefault();
              void handleSend();
            }
          }}
        />

        {logConfirmOpen ? (
          <div className='support-chat-log-confirm flex items-start justify-between gap-12px rd-10px border border-solid border-[var(--color-border-2)] bg-fill-1 px-10px py-8px'>
            <div className='min-w-0 text-12px text-t-secondary leading-18px'>
              {t('common.supportChat.uploadLogsConfirm', {
                defaultValue: '上传日志，仅用于排查问题，可能包含对话记录、设备信息等数据',
              })}
            </div>
            <div className='flex shrink-0 items-center gap-6px'>
              <button
                type='button'
                className='support-chat-composer__text-action'
                onClick={() => setLogConfirmOpen(false)}
                disabled={preparingLogs}
              >
                {t('common.cancel', { defaultValue: '取消' })}
              </button>
              <button
                type='button'
                className='support-chat-composer__text-action support-chat-composer__text-action--primary'
                onClick={() => void prepareAndUploadLogs()}
                disabled={preparingLogs}
              >
                {preparingLogs
                  ? t('common.supportChat.uploadLogsPreparing', { defaultValue: '准备中…' })
                  : t('common.confirm', { defaultValue: '确定' })}
              </button>
            </div>
          </div>
        ) : null}

        <div className='support-chat-composer__toolbar h-32px flex items-center justify-between gap-12px'>
          <div className='min-w-0 flex items-center gap-10px'>
            <Trigger
              popup={() => attachmentMenu}
              trigger='click'
              position='tl'
              popupVisible={attachmentMenuOpen}
              onVisibleChange={setAttachmentMenuOpen}
              clickToClose
              popupAlign={{ bottom: 8 }}
            >
              <button
                type='button'
                data-button-shape='circle'
                className='size-32px shrink-0 flex items-center justify-center rd-full border-none bg-fill-2 text-t-secondary cursor-pointer transition-colors hover:bg-fill-3 disabled:opacity-50 disabled:cursor-not-allowed'
                disabled={disabled || sending || preparingLogs}
                aria-label={t('common.supportChat.addAttachment', { defaultValue: '添加附件' })}
              >
                <span className='support-chat-composer__icon'>
                  <Plus theme='outline' size='18' strokeWidth={3} />
                </span>
              </button>
            </Trigger>
            <span
              className={
                charCount > MAX_SUPPORT_MESSAGE_CHARS
                  ? 'text-12px text-danger'
                  : 'text-12px text-t-tertiary'
              }
            >
              {charCount} / {MAX_SUPPORT_MESSAGE_CHARS}
            </span>
          </div>

          <button
            type='button'
            data-button-shape='circle'
            className='support-chat-composer__send size-32px shrink-0 flex items-center justify-center rd-full border-none cursor-pointer transition-opacity hover:opacity-90 active:opacity-80 disabled:bg-fill-3 disabled:text-t-tertiary disabled:cursor-not-allowed'
            disabled={!canSend}
            onClick={() => void handleSend()}
            aria-label={t('common.supportChat.send', { defaultValue: '发送' })}
          >
            <span className='support-chat-composer__icon'>
              <svg
                className='support-chat-send-arrow block size-16px'
                viewBox='0 0 16 16'
                fill='none'
                aria-hidden
              >
                <path
                  d='M3.5 7.25 8 3l4.5 4.25M8 3v8.5'
                  stroke='currentColor'
                  strokeWidth='1.5'
                  strokeLinecap='round'
                  strokeLinejoin='round'
                />
              </svg>
            </span>
          </button>
        </div>
      </div>
    </div>
  );
};

export default SupportMessageComposer;
