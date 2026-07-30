/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Message, Modal, Trigger } from '@arco-design/web-react';
import { FileText, ImageFiles, Plus } from '@icon-park/react';
import React, { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ICloudImAttachmentPayload } from '@/common/adapter/ipcBridge';
import { useCloudAuth } from '@/renderer/hooks/context/CloudAuthContext';
import { supportChatApi } from '../api/supportChatApi';
import { collectSupportDeviceInfo } from '../collectSupportDeviceInfo';
import { collectSupportLogUserInfo } from '../collectSupportLogUserInfo';

const MAX_CHARS = 4000;
const MAX_IMAGES = 4;
const MAX_IMAGE_BYTES = 5 * 1024 * 1024;
const ACCEPTED_IMAGE_TYPES = new Set(['image/png', 'image/jpeg']);
const DEFAULT_LOG_CONTENT = '附上日志，请协助排查';

type ImagePreviewItem = {
  id: string;
  url: string;
  file: File;
};

type SupportMessageComposerProps = {
  disabled?: boolean;
  onSend: (content: string, logPayload?: ICloudImAttachmentPayload) => Promise<void>;
  /** 同步挂 pending 气泡，上传/发送在 provider 后台完成。 */
  onSendImages: (params: {
    content: string;
    images: { file: File; fileName: string; previewUrl: string }[];
  }) => void;
};

const isAcceptedImage = (file: File) => {
  if (ACCEPTED_IMAGE_TYPES.has(file.type)) return true;
  const name = file.name.toLowerCase();
  return name.endsWith('.png') || name.endsWith('.jpg') || name.endsWith('.jpeg');
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
  const [imagePreviews, setImagePreviews] = useState<ImagePreviewItem[]>([]);
  const [preparingLogs, setPreparingLogs] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const imagePreviewsRef = useRef(imagePreviews);
  const charCount = Array.from(value).length;
  const canSend =
    !disabled &&
    !sending &&
    !preparingLogs &&
    (charCount > 0 || imagePreviews.length > 0) &&
    charCount <= MAX_CHARS;
  const canAddImages = imagePreviews.length < MAX_IMAGES;

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
      for (const item of imagePreviewsRef.current) {
        URL.revokeObjectURL(item.url);
      }
    };
  }, []);

  const removeImagePreview = (id: string) => {
    setImagePreviews((current) => {
      const target = current.find((item) => item.id === id);
      if (target) {
        URL.revokeObjectURL(target.url);
      }
      return current.filter((item) => item.id !== id);
    });
  };

  const handleSend = async () => {
    if (!canSend) return;
    const trimmed = value.trim();
    const images = imagePreviews;
    if (!trimmed && images.length === 0) return;

    if (images.length > 0) {
      // 秒上屏：预览所有权交给 pending 气泡，立即清空输入区；上传/发送由 provider 后台处理。
      onSendImages({
        content: trimmed,
        images: images.map((item) => ({
          file: item.file,
          fileName: item.file.name,
          previewUrl: item.url,
        })),
      });
      setValue('');
      setImagePreviews([]);
      if (fileInputRef.current) {
        fileInputRef.current.value = '';
      }
      return;
    }

    setSending(true);
    try {
      await onSend(trimmed);
      setValue('');
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
      await onSend(content, {
        ...(uploaded.url ? { url: uploaded.url } : {}),
        name: uploaded.name || packed.fileName,
        contentType: uploaded.contentType || 'application/zip',
        byteSize: uploaded.byteSize || packed.byteSize,
        ...(uploaded.objectKey ? { objectKey: uploaded.objectKey } : {}),
        account,
        device,
      });
      Message.success(
        t('common.supportChat.uploadLogsReady', {
          defaultValue: '日志已上传并发送',
        })
      );
    } catch (error) {
      Message.error(
        error instanceof Error && error.message
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
    Modal.confirm({
      title: t('common.supportChat.uploadLogs', { defaultValue: '上传日志' }),
      content: t('common.supportChat.uploadLogsConfirm', {
        defaultValue: '上传日志，仅用于排查问题，可能包含对话记录、设备信息等数据',
      }),
      okText: t('common.confirm', { defaultValue: '确定' }),
      cancelText: t('common.cancel', { defaultValue: '取消' }),
      onOk: () => prepareAndUploadLogs(),
    });
  };

  const openImagePicker = () => {
    setAttachmentMenuOpen(false);
    if (!canAddImages) {
      Message.warning(
        t('common.supportChat.imageLimitReached', {
          defaultValue: '最多上传 4 张图片',
          count: MAX_IMAGES,
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

    const remaining = MAX_IMAGES - imagePreviews.length;
    if (remaining <= 0) {
      Message.warning(
        t('common.supportChat.imageLimitReached', {
          defaultValue: '最多上传 4 张图片',
          count: MAX_IMAGES,
        })
      );
      return;
    }

    const accepted: ImagePreviewItem[] = [];
    let rejected = false;
    let truncated = false;

    for (const file of files) {
      if (accepted.length >= remaining) {
        truncated = true;
        break;
      }
      if (!isAcceptedImage(file) || file.size > MAX_IMAGE_BYTES) {
        rejected = true;
        continue;
      }
      accepted.push({
        id: `${file.name}-${file.size}-${file.lastModified}-${Math.random().toString(36).slice(2)}`,
        url: URL.createObjectURL(file),
        file,
      });
    }

    if (accepted.length > 0) {
      setImagePreviews((current) => [...current, ...accepted].slice(0, MAX_IMAGES));
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
          count: MAX_IMAGES,
        })
      );
    }
  };

  const attachmentMenu = (
    <div className='w-148px p-4px rd-10px border border-solid border-[var(--color-border-2)] bg-[var(--dialog-fill-0)] shadow-lg'>
      <button
        type='button'
        className='w-full h-32px px-8px flex items-center gap-8px rd-7px border-none bg-transparent text-13px leading-18px text-t-primary cursor-pointer transition-colors hover:bg-fill-2'
        onClick={openImagePicker}
      >
        <ImageFiles
          theme='outline'
          size='16'
          fill='currentColor'
          className='block shrink-0 text-t-secondary leading-none'
          style={{ lineHeight: 0 }}
        />
        <span>{t('common.supportChat.uploadImage', { defaultValue: '上传图片' })}</span>
      </button>
      <button
        type='button'
        className='w-full h-32px px-8px flex items-center gap-8px rd-7px border-none bg-transparent text-13px leading-18px text-t-primary cursor-pointer transition-colors hover:bg-fill-2'
        onClick={showUploadLogsConfirm}
      >
        <FileText
          theme='outline'
          size='16'
          fill='currentColor'
          className='block shrink-0 text-t-secondary leading-none'
          style={{ lineHeight: 0 }}
        />
        <span>{t('common.supportChat.uploadLogs', { defaultValue: '上传日志' })}</span>
      </button>
    </div>
  );

  return (
    <div className='border-t border-[var(--color-border-2)] flex flex-col gap-6px'>
      <input
        ref={fileInputRef}
        type='file'
        accept='image/png,image/jpeg,.png,.jpg,.jpeg'
        multiple
        className='hidden'
        onChange={handleImageSelected}
      />

      <div className='box-border w-full min-h-104px px-6px pt-6px pb-4px flex flex-col gap-0 rd-20px border border-solid border-[var(--color-border-2)] bg-bg-1 transition-colors focus-within:border-primary'>
        {imagePreviews.length > 0 ? (
          <div className='mb-6px flex flex-wrap gap-8px'>
            {imagePreviews.map((item) => (
              <div key={item.id} className='relative w-fit'>
                <img
                  src={item.url}
                  alt=''
                  className='block size-72px object-cover rd-10px border border-solid border-[var(--color-border-2)]'
                />
                <button
                  type='button'
                  className='absolute -top-6px -right-6px size-20px flex items-center justify-center rd-full border-none bg-[rgba(0,0,0,0.72)] text-white cursor-pointer shadow-sm hover:bg-[rgba(0,0,0,0.86)]'
                  onClick={() => removeImagePreview(item.id)}
                  disabled={disabled || sending || preparingLogs}
                  aria-label={t('common.supportChat.removeImage', { defaultValue: '移除图片' })}
                >
                  <svg
                    className='block size-20px'
                    viewBox='0 0 20 20'
                    fill='none'
                    aria-hidden
                  >
                    <path
                      d='M5 5l10 10M15 5 5 15'
                      stroke='currentColor'
                      strokeWidth='1.8'
                      strokeLinecap='round'
                    />
                  </svg>
                </button>
              </div>
            ))}
          </div>
        ) : null}

        <textarea
          ref={textareaRef}
          className='box-border w-full min-h-60px max-h-120px overflow-y-auto resize-none border-none bg-transparent px-2px py-2px text-14px leading-22px text-t-primary outline-none disabled:opacity-60 disabled:cursor-not-allowed'
          rows={2}
          value={value}
          disabled={disabled || sending || preparingLogs}
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

        <div className='h-32px flex items-center justify-between gap-12px'>
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
                className='size-28px shrink-0 flex items-center justify-center rd-full border-none bg-fill-2 text-t-secondary cursor-pointer transition-colors hover:bg-fill-3 disabled:opacity-50 disabled:cursor-not-allowed'
                disabled={disabled || sending || preparingLogs}
                aria-label={t('common.supportChat.addAttachment', { defaultValue: '添加附件' })}
              >
                <Plus theme='outline' size='18' strokeWidth={3} />
              </button>
            </Trigger>
            <span
              className={
                charCount > MAX_CHARS ? 'text-12px text-danger' : 'text-12px text-t-tertiary'
              }
            >
              {charCount} / {MAX_CHARS}
            </span>
          </div>

          <button
            type='button'
            data-button-shape='circle'
            className='size-28px shrink-0 flex items-center justify-center rd-full border-none bg-[var(--text-primary)] text-white cursor-pointer transition-opacity hover:opacity-90 active:opacity-80 disabled:bg-fill-3 disabled:text-t-tertiary disabled:cursor-not-allowed'
            disabled={!canSend}
            onClick={() => void handleSend()}
            aria-label={t('common.supportChat.send', { defaultValue: '发送' })}
          >
            <svg
              className='support-chat-send-arrow size-16px'
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
          </button>
        </div>
      </div>
    </div>
  );
};

export default SupportMessageComposer;
