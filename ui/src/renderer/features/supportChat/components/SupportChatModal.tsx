/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { useTranslation } from 'react-i18next';
import { Close } from '@icon-park/react';
import type { ICloudImAttachmentPayload } from '@/common/adapter/ipcBridge';
import NomiModal from '@renderer/components/base/NomiModal';
import { useSupportChat } from '../SupportChatProvider';
import type { SupportOutgoingImage } from '../SupportChatProvider';
import type { SupportChatState } from '../api/supportChatTypes';
import SupportMessageComposer from './SupportMessageComposer';
import SupportMessageList from './SupportMessageList';

export type SupportChatModalViewProps = {
  state: SupportChatState;
  visible: boolean;
  closeSupportChat: () => void;
  openSupportChat: () => void;
  sendMessage: (content: string, logPayload?: ICloudImAttachmentPayload) => Promise<boolean>;
  sendImages: (params: { content: string; images: SupportOutgoingImage[] }) => boolean;
  retryMessage: (clientMsgId: string) => Promise<void>;
  loadOlder: () => Promise<boolean>;
  composerDisabled?: boolean;
};

export const SupportChatModalView: React.FC<SupportChatModalViewProps> = ({
  state,
  visible,
  closeSupportChat,
  openSupportChat,
  sendMessage,
  sendImages,
  retryMessage,
  loadOlder,
  composerDisabled = false,
}) => {
  const { t } = useTranslation();

  return (
    <NomiModal
      visible={visible}
      header={{
        title: t('common.supportChat.title', { defaultValue: '联系客服' }),
        showClose: true,
        closeIcon: <Close size={18} fill='currentColor' className='block' />,
        className: 'support-chat-modal__header',
      }}
      footer={null}
      className='support-chat-modal w-[min(600px,calc(100vw-32px))] max-w-600px rd-16px'
      alignCenter={false}
      wrapClassName='support-chat-modal__wrapper'
      style={{
        width: 'min(600px, calc(100vw - 32px))',
        maxWidth: 'min(600px, calc(100vw - 32px))',
        height: 'min(680px, calc(100dvh - 32px))',
        maxHeight: 'min(680px, calc(100dvh - 32px))',
      }}
      contentStyle={{ padding: 0, overflow: 'hidden' }}
      unmountOnExit
      onCancel={closeSupportChat}
    >
      <div
        className={`support-chat-modal__body support-chat-modal__body--${state.status} flex min-h-0 h-full flex-col`}
      >
        {state.status === 'ready' && state.syncWarning ? (
          <div
            className='support-chat-modal__sync-warning border-b border-[var(--color-border-2)] px-12px py-8px text-12px text-warning-6 leading-18px'
            role='status'
          >
            {t('common.supportChat.syncWarning', {
              defaultValue: '消息同步暂时中断，正在重试',
            })}
          </div>
        ) : null}

        {state.status === 'loading' ? (
          <div
            className='support-chat-modal__status support-chat-modal__status--loading flex flex-col gap-12px animate-pulse px-16px py-16px'
            role='status'
            aria-busy='true'
          >
            <span className='sr-only'>{t('common.loading', { defaultValue: '请稍候…' })}</span>
            <div className='h-40px w-60% rd-12px rd-bl-4px bg-fill-2' />
            <div className='h-40px w-45% rd-12px rd-br-4px bg-fill-2 self-end' />
            <div className='h-40px w-55% rd-12px rd-bl-4px bg-fill-2' />
          </div>
        ) : null}

        {state.status === 'ready' ? (
          <>
            <SupportMessageList
              messages={state.messages}
              onLoadOlder={loadOlder}
              onRetry={(clientMsgId) => {
                void retryMessage(clientMsgId);
              }}
            />
            <SupportMessageComposer
              disabled={composerDisabled}
              onSend={(content, logPayload) => sendMessage(content, logPayload)}
              onSendImages={sendImages}
            />
          </>
        ) : null}

        {state.status === 'error' ? (
          <div className='support-chat-modal__status support-chat-modal__status--error flex flex-col items-center justify-center text-center gap-8px'>
            <div className='text-14px font-medium text-t-primary'>
              {t('common.supportChat.connectionError', {
                defaultValue: '暂时无法连接客服',
              })}
            </div>
            <div className='text-12px text-t-secondary leading-18px'>{state.message}</div>
            <button
              type='button'
              className='support-chat-modal__action mt-8px h-32px px-16px rd-8px border-none text-13px font-medium cursor-pointer transition-opacity hover:opacity-90 active:opacity-80'
              onClick={() => openSupportChat()}
            >
              {t('common.supportChat.retry', { defaultValue: '重试' })}
            </button>
          </div>
        ) : null}

        {state.status === 'auth-required' ? (
          <div className='support-chat-modal__status support-chat-modal__status--auth-required flex flex-col items-center justify-center text-center gap-8px'>
            <div className='text-14px font-medium text-t-primary'>
              {t('common.supportChat.authRequired', {
                defaultValue: '登录状态已失效',
              })}
            </div>
            <button
              type='button'
              className='support-chat-modal__action mt-8px h-32px px-16px rd-8px border-none text-13px font-medium cursor-pointer transition-opacity hover:opacity-90 active:opacity-80'
              onClick={() => openSupportChat()}
            >
              {t('common.supportChat.relogin', { defaultValue: '重新登录' })}
            </button>
          </div>
        ) : null}
      </div>
    </NomiModal>
  );
};

const SupportChatModal: React.FC = () => {
  const {
    state,
    modalOpen,
    closeSupportChat,
    openSupportChat,
    sendMessage,
    sendImages,
    retryMessage,
    loadOlder,
  } = useSupportChat();

  return (
    <SupportChatModalView
      state={state}
      visible={modalOpen}
      closeSupportChat={closeSupportChat}
      openSupportChat={openSupportChat}
      sendMessage={sendMessage}
      sendImages={sendImages}
      retryMessage={retryMessage}
      loadOlder={loadOlder}
    />
  );
};

export default SupportChatModal;
