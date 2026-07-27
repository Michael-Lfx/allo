/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { useTranslation } from 'react-i18next';
import ModalWrapper from '@renderer/components/base/ModalWrapper';
import { useSupportChat } from '../SupportChatProvider';
import SupportMessageComposer from './SupportMessageComposer';
import SupportMessageList from './SupportMessageList';

const SupportChatModal: React.FC = () => {
  const { t } = useTranslation();
  const {
    state,
    closeSupportChat,
    openSupportChat,
    sendMessage,
    sendImages,
    retryMessage,
    loadOlder,
  } = useSupportChat();
  const visible = state.status !== 'closed';

  return (
    <ModalWrapper
      visible={visible}
      title={t('common.supportChat.title', { defaultValue: '联系客服' })}
      footer={null}
      className='support-chat-modal w-[min(420px,calc(100vw-24px))] max-w-420px rd-16px'
      style={{ maxHeight: 'min(620px, calc(100vh - 48px))' }}
      onCancel={closeSupportChat}
    >
      <div className='flex flex-col' style={{ height: 'min(540px, calc(100dvh - 160px))' }}>
        {state.status === 'ready' && state.syncWarning ? (
          <div className='border-b border-[var(--color-border-2)] text-12px text-warning-6 leading-18px'>
            {t('common.supportChat.syncWarning', {
              defaultValue: '消息同步暂时中断，正在重试',
            })}
          </div>
        ) : null}

        {state.status === 'loading' ? (
          <div className='flex-1 flex flex-col gap-12px animate-pulse'>
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
              onSend={async (content, logPayload) => {
                await sendMessage(content, logPayload);
              }}
              onSendImages={sendImages}
            />
          </>
        ) : null}

        {state.status === 'error' ? (
          <div className='flex-1 flex flex-col items-center justify-center text-center gap-8px'>
            <div className='text-14px font-medium text-t-primary'>
              {t('common.supportChat.connectionError', {
                defaultValue: '暂时无法连接客服',
              })}
            </div>
            <div className='text-12px text-t-secondary leading-18px'>{state.message}</div>
            <button
              type='button'
              className='mt-8px h-32px px-16px rd-8px border-none bg-primary text-white text-13px font-medium cursor-pointer transition-opacity hover:opacity-90 active:opacity-80'
              onClick={() => openSupportChat()}
            >
              {t('common.supportChat.retry', { defaultValue: '重试' })}
            </button>
          </div>
        ) : null}

        {state.status === 'auth-required' ? (
          <div className='flex-1 flex flex-col items-center justify-center text-center gap-8px'>
            <div className='text-14px font-medium text-t-primary'>
              {t('common.supportChat.authRequired', {
                defaultValue: '登录状态已失效',
              })}
            </div>
            <button
              type='button'
              className='mt-8px h-32px px-16px rd-8px border-none bg-primary text-white text-13px font-medium cursor-pointer transition-opacity hover:opacity-90 active:opacity-80'
              onClick={() => openSupportChat()}
            >
              {t('common.supportChat.relogin', { defaultValue: '重新登录' })}
            </button>
          </div>
        ) : null}
      </div>
    </ModalWrapper>
  );
};

export default SupportChatModal;
