/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from 'react';
import { Modal } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@/common';
import { isAuthExpiredHttpError } from '@/common/adapter/httpBridge';
import type {
  ICloudImAttachmentPayload,
  ICloudImConversation,
  ICloudImLogUploadResponse,
} from '@/common/adapter/ipcBridge';
import { useCloudAuth } from '@/renderer/hooks/context/CloudAuthContext';
import ErrorDiagnosticContent from '@/renderer/components/base/ErrorDiagnosticContent';
import { buildAgentErrorDiagnostic } from '@/renderer/utils/ui/errorDiagnostics';
import { supportChatApi } from './api/supportChatApi';
import { collectSupportDeviceInfo } from './collectSupportDeviceInfo';
import { collectSupportLogUserInfo } from './collectSupportLogUserInfo';
import {
  buildConversationErrorReportMetadata,
  type ConversationErrorReportContext,
} from './conversationErrorReport';
import type { SupportChatState, SupportMessage, SupportPendingMessage } from './api/supportChatTypes';
import {
  collectNotifiableSysUserMessages,
  readNotifiedSeq,
  truncateNotificationBody,
  writeNotifiedSeq,
} from './supportNotifications';
import { createSupportPollController } from './supportPollController';
import { createPendingMessage } from './state/supportMessageMerge';
import { supportImagePreviewCache } from './state/supportImagePreviewCache';
import { initialSupportChatState, supportChatReducer } from './state/supportChatReducer';
import SupportChatModal from './components/SupportChatModal';

export type SupportOutgoingImage = {
  file: Blob;
  fileName: string;
  previewUrl: string;
};

type SupportChatContextValue = {
  openSupportChat: () => void;
  closeSupportChat: () => void;
  state: SupportChatState;
  hasUnread: boolean;
  unreadCount: number;
  sendMessage: (content: string, logPayload?: ICloudImAttachmentPayload) => Promise<void>;
  reportConversationError: (context: ConversationErrorReportContext) => void;
  /** 图片秒上屏：同步挂 pending 气泡，上传/发送全部在后台进行。 */
  sendImages: (params: { content: string; images: SupportOutgoingImage[] }) => void;
  retryMessage: (clientMsgId: string) => Promise<void>;
  /** 加载更早的历史消息；返回是否真的加载到了新内容（供列表做滚动锚点补偿）。 */
  loadOlder: () => Promise<boolean>;
};

const SupportChatContext = createContext<SupportChatContextValue | null>(null);

/** 首屏只拉最近 20 条，减少首次渲染体积（含图片时尤为明显）。 */
const FIRST_SCREEN_MESSAGE_LIMIT = 20;
/** 上滑加载历史每页 20 条。 */
const OLDER_PAGE_MESSAGE_LIMIT = 20;

function maxServerSeq(messages: SupportMessage[]): number {
  let max = 0;
  for (const item of messages) {
    if (item.kind === 'server' && item.message.seq > max) {
      max = item.message.seq;
    }
  }
  return max;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message) return error.message;
  return '暂时无法连接客服';
}

function buildImagePayload(
  uploaded: ICloudImLogUploadResponse,
  fallback: { fileName: string; contentType?: string; byteSize: number }
): ICloudImAttachmentPayload {
  return {
    ...(uploaded.url ? { url: uploaded.url } : {}),
    ...(uploaded.objectKey ? { objectKey: uploaded.objectKey } : {}),
    name: uploaded.name || fallback.fileName,
    contentType: uploaded.contentType || fallback.contentType || 'image/png',
    byteSize: uploaded.byteSize || fallback.byteSize,
  };
}

export const SupportChatProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const { t } = useTranslation();
  const { status: cloudStatus, whoami } = useCloudAuth();
  const [state, dispatch] = useReducer(supportChatReducer, initialSupportChatState);
  const [modalOpen, setModalOpen] = useState(false);
  const stateRef = useRef(state);
  const modalOpenRef = useRef(modalOpen);
  // 串行发送队列：保证多条消息（含后台图片）按入队顺序发送，不丢弃。
  const sendQueueRef = useRef<Promise<void>>(Promise.resolve());
  const loadingOlderRef = useRef(false);
  // 会话快照缓存：关闭弹窗后保留，再次打开直接渲染，后台增量刷新。
  const snapshotRef = useRef<{
    conversation: ICloudImConversation;
    messages: SupportMessage[];
  } | null>(null);
  const pollBridgeRef = useRef<{
    setModalOpen?: (open: boolean) => void;
    setAfterSeq?: (seq: number) => void;
  }>({});

  stateRef.current = state;
  modalOpenRef.current = modalOpen;

  useEffect(() => {
    if (state.status === 'ready') {
      snapshotRef.current = { conversation: state.conversation, messages: state.messages };
    }
  }, [state]);

  // 并行拉取会话 + 最近消息（两次云端往返合并为 1 个 RTT）。首屏 20 条，更早的上滑分页。
  const fetchConversationSnapshot = useCallback(async () => {
    const [conversation, listed] = await Promise.all([
      supportChatApi.getConversation(),
      supportChatApi.listMessages({ limit: FIRST_SCREEN_MESSAGE_LIMIT }),
    ]);
    return {
      conversation,
      messages: listed.list.map((message) => ({ kind: 'server' as const, message })),
    };
  }, []);

  const notifyNewMessages = useCallback(
    (messages: Parameters<typeof collectNotifiableSysUserMessages>[0]) => {
      const userId = whoami?.userId;
      if (!userId || typeof localStorage === 'undefined') return;

      const focused =
        typeof document === 'undefined'
          ? true
          : document.visibilityState === 'visible' && document.hasFocus();
      const modalVisibleAndFocused = modalOpenRef.current && focused;
      const lastNotifiedSeq = readNotifiedSeq(localStorage, userId);
      const { toNotify, nextNotifiedSeq } = collectNotifiableSysUserMessages(
        messages,
        lastNotifiedSeq,
        { modalVisibleAndFocused }
      );
      if (nextNotifiedSeq > lastNotifiedSeq) {
        writeNotifiedSeq(localStorage, userId, nextNotifiedSeq);
      }
      for (const message of toNotify) {
        void ipcBridge.notification.show
          .invoke({
            title: '客服回复了你',
            body: truncateNotificationBody(message.content),
          })
          .catch(() => {
            // Permission denied or unsupported — badge remains the fallback.
          });
      }
    },
    [whoami?.userId]
  );

  useEffect(() => {
    if (cloudStatus !== 'authenticated') {
      dispatch({ type: 'reset' });
      setModalOpen(false);
      snapshotRef.current = null;
      pollBridgeRef.current = {};
      return;
    }

    const controller = createSupportPollController({
      getConversation: () => supportChatApi.getConversation(),
      listMessagesAfter: async (afterSeq) => {
        const result = await supportChatApi.listMessages({
          afterSeq: afterSeq > 0 ? afterSeq : undefined,
          limit: 50,
        });
        return result.list;
      },
      onUnread: (conversation) => {
        dispatch({
          type: 'set-unread',
          unreadCount: Math.max(0, conversation.userUnreadCount),
        });
      },
      onMessages: (incoming) => {
        if (incoming.length === 0) {
          dispatch({ type: 'sync-warning', syncWarning: false });
          return;
        }
        dispatch({ type: 'messages-merged', incoming });
        notifyNewMessages(incoming);
        const current = stateRef.current;
        if (current.status === 'ready' && modalOpenRef.current) {
          const hasSysUser = incoming.some((item) => item.senderType === 'sys_user');
          if (hasSysUser) {
            const lastSeq = Math.max(
              current.conversation.lastSeq,
              ...incoming.map((item) => item.seq)
            );
            void supportChatApi
              .markRead(lastSeq)
              .then((conversation) => {
                dispatch({ type: 'conversation-updated', conversation });
                dispatch({ type: 'set-unread', unreadCount: 0 });
              })
              .catch(() => {
                dispatch({ type: 'sync-warning', syncWarning: true });
              });
          }
        }
      },
      onError: (error) => {
        if (isAuthExpiredHttpError(error)) {
          dispatch({ type: 'auth-required' });
          return;
        }
        if (stateRef.current.status === 'ready') {
          dispatch({ type: 'sync-warning', syncWarning: true });
        }
      },
      setTimeout: (fn, ms) => window.setTimeout(fn, ms),
      clearTimeout: (id) => window.clearTimeout(id as number),
    });

    pollBridgeRef.current = {
      setModalOpen: (open) => controller.setModalOpen(open),
      setAfterSeq: (seq) => controller.setAfterSeq(seq),
    };

    const onVisibility = () => {
      controller.setVisibility(document.visibilityState === 'hidden' ? 'hidden' : 'visible');
    };
    document.addEventListener('visibilitychange', onVisibility);
    controller.setVisibility(document.visibilityState === 'hidden' ? 'hidden' : 'visible');
    controller.setModalOpen(modalOpenRef.current);
    if (stateRef.current.status === 'ready') {
      controller.setAfterSeq(maxServerSeq(stateRef.current.messages));
    }
    controller.start();

    // 后台预热：登录后先拉一份快照，首次点开弹窗即可秒开。
    if (!snapshotRef.current) {
      void fetchConversationSnapshot()
        .then((snapshot) => {
          if (!snapshotRef.current) {
            snapshotRef.current = snapshot;
          }
        })
        .catch(() => {
          // 预热失败不打扰；打开弹窗时会重新拉取并展示错误。
        });
    }

    return () => {
      document.removeEventListener('visibilitychange', onVisibility);
      pollBridgeRef.current = {};
      controller.dispose();
    };
  }, [cloudStatus, notifyNewMessages, fetchConversationSnapshot]);

  useEffect(() => {
    pollBridgeRef.current.setModalOpen?.(modalOpen);
  }, [modalOpen]);

  useEffect(() => {
    if (state.status === 'ready') {
      pollBridgeRef.current.setAfterSeq?.(maxServerSeq(state.messages));
    }
  }, [state]);

  const openSupportChat = useCallback(() => {
    if (cloudStatus !== 'authenticated') {
      dispatch({ type: 'auth-required' });
      setModalOpen(true);
      return;
    }
    setModalOpen(true);
    // 有快照（预热或上次打开的缓存）就直接渲染，刷新转后台；否则先展示骨架屏。
    const cached = snapshotRef.current;
    if (cached) {
      dispatch({ type: 'ready', conversation: cached.conversation, messages: cached.messages });
    } else {
      dispatch({ type: 'open' });
    }
    void (async () => {
      try {
        const snapshot = await fetchConversationSnapshot();
        if (cached && stateRef.current.status === 'ready') {
          // 已用缓存上屏：增量合并，保留发送中的 pending 气泡。
          dispatch({ type: 'conversation-updated', conversation: snapshot.conversation });
          dispatch({
            type: 'messages-merged',
            incoming: snapshot.messages.map((item) => item.message),
          });
        } else {
          dispatch({ type: 'ready', conversation: snapshot.conversation, messages: snapshot.messages });
        }
        if (snapshot.conversation.userUnreadCount > 0 && snapshot.conversation.lastSeq > 0) {
          // 已读回执后台执行，不阻塞首屏。
          void supportChatApi
            .markRead(snapshot.conversation.lastSeq)
            .then((updated) => {
              dispatch({ type: 'conversation-updated', conversation: updated });
              dispatch({ type: 'set-unread', unreadCount: 0 });
            })
            .catch(() => {
              dispatch({ type: 'sync-warning', syncWarning: true });
            });
        }
      } catch (error) {
        if (isAuthExpiredHttpError(error)) {
          dispatch({ type: 'auth-required' });
          return;
        }
        if (stateRef.current.status === 'ready') {
          // 缓存已展示：刷新失败降级为同步告警，不覆盖已有内容。
          dispatch({ type: 'sync-warning', syncWarning: true });
          return;
        }
        dispatch({ type: 'error', message: errorMessage(error) });
      }
    })();
  }, [cloudStatus, fetchConversationSnapshot]);

  const closeSupportChat = useCallback(() => {
    setModalOpen(false);
    dispatch({ type: 'close' });
  }, []);

  const sendWithClientMsgId = useCallback(
    async (
      clientMsgId: string,
      content: string,
      options?: {
        msgType?: 'text' | 'image';
        payload?: ICloudImAttachmentPayload;
        logPayload?: ICloudImAttachmentPayload;
      }
    ) => {
      const task = sendQueueRef.current.then(async () => {
        try {
          const message = await supportChatApi.sendMessage({
            clientMsgId,
            content,
            msgType: options?.msgType ?? 'text',
            payload: options?.payload,
            logPayload: options?.logPayload,
          });
          dispatch({ type: 'pending-replaced', clientMsgId, message });
        } catch (error) {
          dispatch({ type: 'pending-failed', clientMsgId });
          if (isAuthExpiredHttpError(error)) {
            dispatch({ type: 'auth-required' });
          }
          throw error;
        }
      });
      sendQueueRef.current = task.catch(() => {});
      await task;
    },
    []
  );

  const sendMessage = useCallback(
    async (content: string, logPayload?: ICloudImAttachmentPayload) => {
      const trimmed = content.trim();
      if (!trimmed || stateRef.current.status !== 'ready') return;
      const clientMsgId = crypto.randomUUID();
      const createdAt = new Date().toISOString();
      const pending = createPendingMessage(
        clientMsgId,
        trimmed,
        createdAt,
        'sending',
        logPayload
      );
      dispatch({ type: 'pending-added', message: pending });
      await sendWithClientMsgId(clientMsgId, trimmed, { logPayload });
    },
    [sendWithClientMsgId]
  );

  const reportConversationError = useCallback(
    (context: ConversationErrorReportContext) => {
      if (cloudStatus !== 'authenticated') {
        Message.warning(t('common.supportChat.authRequired'));
        openSupportChat();
        return;
      }

      const clientMsgId = crypto.randomUUID();
      const content = t('common.supportChat.uploadLogsDefaultContent');
      const diagnostic = buildAgentErrorDiagnostic(context.error);
      let preparedLogPayload: ICloudImAttachmentPayload | undefined;

      const prepareLogPayload = async (): Promise<ICloudImAttachmentPayload> => {
        if (preparedLogPayload) return preparedLogPayload;

        const devicePromise = collectSupportDeviceInfo();
        const packed = await supportChatApi.packLogs({
          turnId: context.turnId ?? context.messageId,
        });
        const [uploaded, device] = await Promise.all([
          supportChatApi.uploadLogFromPath({
            zipPath: packed.zipPath,
            fileName: packed.fileName,
          }),
          devicePromise,
        ]);
        preparedLogPayload = {
          ...(uploaded.url ? { url: uploaded.url } : {}),
          ...(uploaded.objectKey ? { objectKey: uploaded.objectKey } : {}),
          name: uploaded.name || packed.fileName,
          contentType: uploaded.contentType || 'application/zip',
          byteSize: uploaded.byteSize || packed.byteSize,
          account: collectSupportLogUserInfo(whoami),
          device,
          report: buildConversationErrorReportMetadata(context),
        };
        return preparedLogPayload;
      };

      Modal.confirm({
        title: t('settings.bugReportTitle'),
        content: (
          <div className='text-13px leading-20px text-t-secondary'>
            <p className='m-0'>{t('settings.bugReportAutoInfo')}</p>
            <p className='m-0 mt-8px'>{t('common.supportChat.uploadLogsConfirm')}</p>
            <ErrorDiagnosticContent diagnostic={diagnostic} className='conversation-error-diagnostic--support' />
          </div>
        ),
        okText: t('settings.bugReportSubmit'),
        cancelText: t('settings.bugReportCancel'),
        onOk: async () => {
          try {
            const logPayload = await prepareLogPayload();
            await sendWithClientMsgId(clientMsgId, content, { logPayload });
            Message.success(t('settings.bugReportSuccess'));
          } catch (error) {
            Message.error(t('settings.bugReportError'));
            throw error;
          }
        },
      });
    },
    [cloudStatus, openSupportChat, sendWithClientMsgId, t, whoami]
  );

  // 图片发送：同步挂出全部 pending 气泡（本地预览秒上屏），
  // 后台并行上传（文档 §4）、按顺序发送（文档 §5.2）；说明文字随最后一张，展示在整组图片之后。
  const sendImages = useCallback(
    (params: { content: string; images: SupportOutgoingImage[] }) => {
      if (stateRef.current.status !== 'ready' || params.images.length === 0) return;
      const caption = params.content.trim();
      const now = Date.now();
      const entries = params.images.map((image, index) => ({
        ...image,
        clientMsgId: crypto.randomUUID(),
        content: index === params.images.length - 1 ? caption : '',
        createdAt: new Date(now + index).toISOString(),
      }));

      for (const entry of entries) {
        supportImagePreviewCache.set(entry.clientMsgId, entry.previewUrl);
        dispatch({
          type: 'pending-added',
          message: createPendingMessage(entry.clientMsgId, entry.content, entry.createdAt, 'sending', undefined, {
            previewUrl: entry.previewUrl,
            file: entry.file,
            fileName: entry.fileName,
          }),
        });
      }

      void (async () => {
        const uploads = entries.map(async (entry) => {
          try {
            const uploaded = await supportChatApi.uploadScreenshot({
              file: entry.file,
              fileName: entry.fileName,
            });
            return {
              entry,
              payload: buildImagePayload(uploaded, {
                fileName: entry.fileName,
                contentType: entry.file.type,
                byteSize: entry.file.size,
              }),
            };
          } catch (error) {
            dispatch({ type: 'pending-failed', clientMsgId: entry.clientMsgId });
            if (isAuthExpiredHttpError(error)) {
              dispatch({ type: 'auth-required' });
            }
            return null;
          }
        });
        for (const uploadTask of uploads) {
          const uploadedItem = await uploadTask;
          if (!uploadedItem) continue;
          const { entry, payload } = uploadedItem;
          // 把 payload 回写到 pending，保证发送失败后重试无需重新上传。
          dispatch({
            type: 'pending-added',
            message: createPendingMessage(entry.clientMsgId, entry.content, entry.createdAt, 'sending', undefined, {
              payload,
              previewUrl: entry.previewUrl,
              file: entry.file,
              fileName: entry.fileName,
            }),
          });
          try {
            await sendWithClientMsgId(entry.clientMsgId, entry.content, {
              msgType: 'image',
              payload,
            });
          } catch {
            // pending-failed 已在 sendWithClientMsgId 内标记，气泡上可重试。
          }
        }
      })();
    },
    [sendWithClientMsgId]
  );

  const retryMessage = useCallback(
    async (clientMsgId: string) => {
      const current = stateRef.current;
      if (current.status !== 'ready') return;
      const pending = current.messages.find(
        (item): item is SupportPendingMessage =>
          item.kind === 'pending' && item.clientMsgId === clientMsgId
      );
      if (!pending) return;
      dispatch({
        type: 'pending-added',
        message: { ...pending, delivery: 'sending' },
      });
      // 图片上传阶段失败的 pending 还没有 payload，重试时先重新上传。
      let payload = pending.payload;
      if (pending.msgType === 'image' && !payload) {
        if (!pending.file) {
          dispatch({ type: 'pending-failed', clientMsgId });
          return;
        }
        try {
          const uploaded = await supportChatApi.uploadScreenshot({
            file: pending.file,
            fileName: pending.fileName || 'screenshot.png',
          });
          payload = buildImagePayload(uploaded, {
            fileName: pending.fileName || 'screenshot.png',
            contentType: pending.file.type,
            byteSize: pending.file.size,
          });
          dispatch({
            type: 'pending-added',
            message: { ...pending, delivery: 'sending', payload },
          });
        } catch (error) {
          dispatch({ type: 'pending-failed', clientMsgId });
          if (isAuthExpiredHttpError(error)) {
            dispatch({ type: 'auth-required' });
          }
          return;
        }
      }
      await sendWithClientMsgId(pending.clientMsgId, pending.content, {
        msgType: pending.msgType ?? 'text',
        payload,
        logPayload: pending.logPayload,
      });
    },
    [sendWithClientMsgId]
  );

  const loadOlder = useCallback(async (): Promise<boolean> => {
    const current = stateRef.current;
    if (current.status !== 'ready' || loadingOlderRef.current) return false;
    const firstServer = current.messages.find((item) => item.kind === 'server');
    if (!firstServer || firstServer.kind !== 'server') return false;
    loadingOlderRef.current = true;
    try {
      const listed = await supportChatApi.listMessages({
        beforeSeq: firstServer.message.seq,
        limit: OLDER_PAGE_MESSAGE_LIMIT,
      });
      if (listed.list.length > 0) {
        dispatch({ type: 'messages-merged', incoming: listed.list });
        return true;
      }
      return false;
    } catch (error) {
      if (isAuthExpiredHttpError(error)) {
        dispatch({ type: 'auth-required' });
      } else {
        dispatch({ type: 'sync-warning', syncWarning: true });
      }
      return false;
    } finally {
      loadingOlderRef.current = false;
    }
  }, []);

  const unreadCount = state.unreadCount;
  const hasUnread = unreadCount > 0;

  const value = useMemo<SupportChatContextValue>(
    () => ({
      openSupportChat,
      closeSupportChat,
      state,
      hasUnread,
      unreadCount,
      sendMessage,
      reportConversationError,
      sendImages,
      retryMessage,
      loadOlder,
    }),
    [
      openSupportChat,
      closeSupportChat,
      state,
      hasUnread,
      unreadCount,
      sendMessage,
      reportConversationError,
      sendImages,
      retryMessage,
      loadOlder,
    ]
  );

  return (
    <SupportChatContext.Provider value={value}>
      {children}
      <SupportChatModal />
    </SupportChatContext.Provider>
  );
};

export function useSupportChat(): SupportChatContextValue {
  const context = useContext(SupportChatContext);
  if (!context) {
    throw new Error('useSupportChat must be used within a SupportChatProvider');
  }
  return context;
}
