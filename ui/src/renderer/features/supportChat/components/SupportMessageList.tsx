/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { SupportMessage } from '../api/supportChatTypes';
import { supportImagePreviewCache } from '../state/supportImagePreviewCache';

type SupportMessageListProps = {
  messages: SupportMessage[];
  /** 返回是否真的加载到了更早的消息。 */
  onLoadOlder: () => Promise<boolean>;
  onRetry: (clientMsgId: string) => void;
};

function formatTime(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return '';
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

function dayKey(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return '';
  return date.toLocaleDateString();
}

type SupportMessageImageProps = {
  localPreviewUrl?: string;
  remoteUrl?: string;
  clientMsgId?: string | null;
  alt: string;
  className: string;
  onClick: () => void;
};

const SupportMessageImage: React.FC<SupportMessageImageProps> = ({
  localPreviewUrl,
  remoteUrl,
  clientMsgId,
  alt,
  className,
  onClick,
}) => {
  const [src, setSrc] = useState(localPreviewUrl || remoteUrl);

  useEffect(() => {
    let cancelled = false;
    setSrc(localPreviewUrl || remoteUrl);

    if (
      !clientMsgId ||
      !localPreviewUrl ||
      !remoteUrl ||
      localPreviewUrl === remoteUrl ||
      !/^https?:\/\//i.test(remoteUrl)
    ) {
      return undefined;
    }

    // Preload the CDN image before releasing the local URL. This keeps the
    // zero-flash transition while giving the cache a deterministic owner.
    const remoteImage = new Image();
    remoteImage.onload = () => {
      if (cancelled) return;
      supportImagePreviewCache.release(clientMsgId);
      setSrc(remoteUrl);
    };
    remoteImage.onerror = () => {
      // Keep the local preview until the message surface is left.
    };
    remoteImage.src = remoteUrl;

    return () => {
      cancelled = true;
      remoteImage.onload = null;
      remoteImage.onerror = null;
    };
  }, [clientMsgId, localPreviewUrl, remoteUrl]);

  useEffect(() => {
    return () => {
      if (clientMsgId) supportImagePreviewCache.release(clientMsgId);
    };
  }, [clientMsgId]);

  if (!src) return null;
  return <img src={src} alt={alt} className={className} onClick={onClick} />;
};

const SupportMessageList: React.FC<SupportMessageListProps> = ({ messages, onLoadOlder, onRetry }) => {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement>(null);
  const stickToBottomRef = useRef(true);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const loadingOlderRef = useRef(false);
  const messagesRef = useRef(messages);
  messagesRef.current = messages;
  // 历史消息前插时的锚点：在 DOM 提交后（useLayoutEffect）同步补偿滚动位置，
  // 避免在旧 scrollHeight 上计算导致跳顶。
  const prependAnchorRef = useRef<{ prevHeight: number; prevTop: number } | null>(null);

  useLayoutEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const anchor = prependAnchorRef.current;
    if (anchor && el.scrollHeight > anchor.prevHeight) {
      prependAnchorRef.current = null;
      el.scrollTop = anchor.prevTop + (el.scrollHeight - anchor.prevHeight);
      return;
    }
    if (stickToBottomRef.current) {
      el.scrollTop = el.scrollHeight;
    }
  }, [messages]);

  useEffect(() => {
    return () => {
      // Pending previews remain owned by their retryable pending message. A
      // server image can safely fall back to its remote URL after this list
      // is closed, so release only those cache entries here.
      for (const item of messagesRef.current) {
        if (item.kind === 'server' && item.message.msgType === 'image') {
          supportImagePreviewCache.release(item.message.clientMsgId);
        }
      }
    };
  }, []);

  const handleScroll = () => {
    const el = containerRef.current;
    if (!el) return;
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    stickToBottomRef.current = distance < 48;
    if (el.scrollTop <= 16 && !loadingOlderRef.current) {
      loadingOlderRef.current = true;
      prependAnchorRef.current = { prevHeight: el.scrollHeight, prevTop: el.scrollTop };
      setLoadingOlder(true);
      void onLoadOlder()
        .then((loaded) => {
          // 没有更早消息时丢弃锚点，防止后续新消息进入时被误补偿。
          if (!loaded) {
            prependAnchorRef.current = null;
          }
        })
        .catch(() => {
          prependAnchorRef.current = null;
        })
        .finally(() => {
          loadingOlderRef.current = false;
          setLoadingOlder(false);
        });
    }
  };

  if (messages.length === 0) {
    return (
      <div className='support-message-list support-message-list--empty min-h-0 overflow-hidden flex items-center justify-center px-16px py-20px'>
        <div className='max-w-260px text-center text-13px text-t-secondary leading-22px'>
          {t('common.supportChat.emptyHint', {
            defaultValue: '请描述你遇到的问题。客服查看后会在这里回复。',
          })}
        </div>
      </div>
    );
  }

  let lastDay = '';
  let lastMinute = '';

  return (
    <div
      ref={containerRef}
      className='support-message-list min-h-0 overflow-auto flex flex-col gap-12px px-16px py-16px'
      onScroll={handleScroll}
      aria-live='polite'
    >
      {loadingOlder ? (
        <div className='text-12px text-t-tertiary text-center py-4px'>
          {t('common.supportChat.loadingOlder', { defaultValue: '加载更早消息…' })}
        </div>
      ) : null}
      {messages.map((item, index) => {
        const createdAt = item.kind === 'server' ? item.message.createdAt : item.createdAt;
        const day = dayKey(createdAt);
        const minute = `${day}-${formatTime(createdAt)}`;
        const showDay = day && day !== lastDay;
        const showTime = minute !== lastMinute;
        lastDay = day || lastDay;
        lastMinute = minute;

        if (item.kind === 'server' && item.message.senderType === 'system') {
          return (
            <React.Fragment key={`sys-${item.message.id}-${item.message.seq}`}>
              {showDay ? (
                <div className='text-11px text-t-tertiary text-center pt-8px pb-4px'>{day}</div>
              ) : null}
              <div className='text-12px text-t-tertiary text-center px-12px py-2px'>
                {item.message.status === 'recalled'
                  ? t('common.supportChat.recalled', { defaultValue: '消息已撤回' })
                  : item.message.content}
              </div>
            </React.Fragment>
          );
        }

        const isUser =
          item.kind === 'pending' ||
          (item.kind === 'server' && item.message.senderType !== 'sys_user');
        const recalled = item.kind === 'server' && item.message.status === 'recalled';
        const content =
          item.kind === 'pending'
            ? item.content
            : recalled
              ? t('common.supportChat.recalled', { defaultValue: '消息已撤回' })
              : item.message.content;
        // 图片消息：pending 用本地预览；服务端优先用本会话缓存的预览（免 CDN 重载闪烁），否则 payload.url。
        const localPreviewUrl = recalled
          ? undefined
          : item.kind === 'pending'
            ? item.msgType === 'image'
              ? item.previewUrl || item.payload?.url
              : undefined
            : item.message.msgType === 'image'
              ? supportImagePreviewCache.get(item.message.clientMsgId)
              : undefined;
        const remoteImageUrl = recalled
          ? undefined
          : item.kind === 'server'
            ? item.message.payload?.url
            : item.payload?.url;
        const imageUrl = localPreviewUrl || remoteImageUrl;
        // 连续己方消息成组：状态行（发送中/已送达）只挂在每组最后一条；失败始终单独显示。
        const next = messages[index + 1];
        const nextIsUser =
          !!next &&
          (next.kind === 'pending' ||
            (next.message.senderType !== 'sys_user' && next.message.senderType !== 'system'));
        const isLastOfUserRun = !nextIsUser;
        const key =
          item.kind === 'pending'
            ? `pending-${item.clientMsgId}`
            : `server-${item.message.id}-${item.message.seq}`;

        return (
          <React.Fragment key={key}>
            {showDay ? (
              <div className='text-11px text-t-tertiary text-center pt-8px pb-4px'>{day}</div>
            ) : null}
            <div
              className={isUser ? 'flex flex-col items-end gap-4px' : 'flex flex-col items-start gap-4px'}
              data-delivery={item.kind === 'pending' ? item.delivery : undefined}
            >
              {showTime ? (
                <div className='text-11px text-t-tertiary px-4px'>{formatTime(createdAt)}</div>
              ) : null}
              {imageUrl ? (
                <SupportMessageImage
                  localPreviewUrl={localPreviewUrl}
                  remoteUrl={remoteImageUrl}
                  clientMsgId={item.kind === 'server' ? item.message.clientMsgId : undefined}
                  alt={t('common.supportChat.imageMessage', { defaultValue: '图片' })}
                  className='max-w-160px max-h-160px rd-12px border border-solid border-[var(--color-border-2)] object-cover cursor-zoom-in bg-fill-1'
                  onClick={() => {
                    // 本地预览 URL 不外开；远端 CDN 图片新开查看（文档 §3.1）。
                    if (remoteImageUrl && /^https?:\/\//.test(remoteImageUrl)) {
                      window.open(remoteImageUrl, '_blank', 'noopener,noreferrer');
                    }
                  }}
                />
              ) : null}
              {imageUrl && content ? (
                <div
                  className={
                    isUser
                      ? 'support-message-list__bubble max-w-[85%] px-12px py-8px rd-12px rd-br-4px bg-primary text-white text-13px leading-20px whitespace-pre-wrap break-words'
                      : 'support-message-list__bubble max-w-[85%] px-12px py-8px rd-12px rd-bl-4px bg-fill-2 text-t-primary text-13px leading-20px whitespace-pre-wrap break-words'
                  }
                >
                  {content}
                </div>
              ) : null}
              {!imageUrl ? (
                <div
                  className={
                    isUser
                      ? 'support-message-list__bubble max-w-[85%] px-12px py-8px rd-12px rd-br-4px bg-primary text-white text-13px leading-20px whitespace-pre-wrap break-words'
                      : 'support-message-list__bubble max-w-[85%] px-12px py-8px rd-12px rd-bl-4px bg-fill-2 text-t-primary text-13px leading-20px whitespace-pre-wrap break-words'
                  }
                >
                  {content}
                </div>
              ) : null}
              {item.kind === 'pending' ? (
                item.delivery === 'failed' ? (
                  <div className='text-11px text-t-tertiary px-4px flex items-center gap-8px'>
                    {t('common.supportChat.sendFailed', { defaultValue: '发送失败' })}
                    <button
                      type='button'
                      className='text-primary border-none bg-transparent cursor-pointer p-0 transition-opacity hover:opacity-80'
                      onClick={() => onRetry(item.clientMsgId)}
                    >
                      {t('common.supportChat.retry', { defaultValue: '重试' })}
                    </button>
                  </div>
                ) : isLastOfUserRun ? (
                  <div className='text-11px text-t-tertiary px-4px'>
                    {t('common.supportChat.sending', { defaultValue: '发送中' })}
                  </div>
                ) : null
              ) : isUser && isLastOfUserRun ? (
                <div className='text-11px text-t-tertiary px-4px'>
                  {t('common.supportChat.sent', { defaultValue: '已送达' })}
                </div>
              ) : null}
            </div>
          </React.Fragment>
        );
      })}
    </div>
  );
};

export default SupportMessageList;
