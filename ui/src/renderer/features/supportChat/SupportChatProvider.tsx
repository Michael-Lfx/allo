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
import { AppMessage as Message } from '@/renderer/components/notifications';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@/common';
import { isAuthExpiredHttpError } from '@/common/adapter/httpBridge';
import { configService } from '@/common/config/configService';
import type {
  ICloudImAttachmentPayload,
  ICloudImConversation,
} from '@/common/adapter/ipcBridge';
import { useCloudAuth } from '@/renderer/hooks/context/CloudAuthContext';
import { supportChatApi } from './api/supportChatApi';
import { collectSupportDeviceInfo } from './collectSupportDeviceInfo';
import { collectSupportLogUserInfo } from './collectSupportLogUserInfo';
import {
  getConversationErrorReportContextKey,
  type ConversationErrorReportDraft,
  type ConversationErrorReportContext,
  type ConversationErrorReportSubmitResult,
} from './conversationErrorReport';
import { submitConversationErrorReport as submitConversationErrorReportFlow } from './conversationErrorReportSubmission';
import {
  MAX_SUPPORT_MESSAGE_CHARS,
  type SupportChatState,
  type SupportMessage,
  type SupportPendingMessage,
  type SupportSendOutcome,
} from './api/supportChatTypes';
import {
  collectNotifiableSysUserMessages,
  readNotifiedSeq,
  truncateNotificationBody,
  writeNotifiedSeq,
} from './supportNotifications';
import { createSupportPollController } from './supportPollController';
import { createPendingMessage, mergeServerMessages } from './state/supportMessageMerge';
import { supportImagePreviewCache } from './state/supportImagePreviewCache';
import {
  buildSupportImagePayload,
  getSupportImageContentType,
  MAX_SUPPORT_IMAGE_BYTES,
  MAX_SUPPORT_IMAGES,
  normalizeSupportImageFile,
} from './supportImageAttachments';
import { isSupportSessionCurrent } from './supportSession';
import { initialSupportChatState, supportChatReducer } from './state/supportChatReducer';
import SupportChatModal from './components/SupportChatModal';
import { supportAttentionId, supportNotifyDeepLink } from '@renderer/hooks/system/desktopNotifyDeepLink';
import ConversationErrorReportModal from './components/ConversationErrorReportModal';

export type SupportOutgoingImage = {
  file: Blob;
  fileName: string;
  previewUrl: string;
};

type SupportChatContextValue = {
  openSupportChat: () => void;
  closeSupportChat: () => void;
  modalOpen: boolean;
  state: SupportChatState;
  hasUnread: boolean;
  unreadCount: number;
  sendMessage: (content: string, logPayload?: ICloudImAttachmentPayload) => Promise<boolean>;
  reportConversationError: (context: ConversationErrorReportContext) => void;
  /** 图片秒上屏：同步挂 pending 气泡，上传/发送全部在后台进行。 */
  sendImages: (params: { content: string; images: SupportOutgoingImage[] }) => boolean;
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

export const SupportChatProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const { t } = useTranslation();
  const { status: cloudStatus, authState, whoami } = useCloudAuth();
  const authAccountId =
    cloudStatus === 'authenticated'
      ? authState.phase === 'authenticated'
        ? authState.accountId
        : authState.phase === 'offline'
          ? authState.previousAccountId || whoami?.userId || 'authenticated-account'
          : whoami?.userId || 'authenticated-account'
      : null;
  const [state, dispatch] = useReducer(supportChatReducer, initialSupportChatState);
  const [modalOpen, setModalOpen] = useState(false);
  const [conversationErrorReportContext, setConversationErrorReportContext] =
    useState<ConversationErrorReportContext | null>(null);
  const [reportModalInstanceKey, setReportModalInstanceKey] = useState(0);
  const stateRef = useRef(state);
  const modalOpenRef = useRef(modalOpen);
  const cloudStatusRef = useRef(cloudStatus);
  const authAccountIdRef = useRef<string | null>(authAccountId);
  const supportSessionGenerationRef = useRef(0);
  const supportOpenGenerationRef = useRef(0);
  const reportGenerationRef = useRef(0);
  const reportContextKeyRef = useRef<string | null>(null);
  const supportAccountIdRef = useRef<string | null>(null);
  // 串行发送队列：保证多条消息（含后台图片）按入队顺序发送，不丢弃。
  const sendQueueRef = useRef<Promise<void>>(Promise.resolve());
  const loadingOlderRef = useRef(false);
  // 会话快照缓存：关闭弹窗后保留，再次打开直接渲染，后台增量刷新。
  const snapshotRef = useRef<{
    conversation: ICloudImConversation;
    messages: SupportMessage[];
  } | null>(null);
  const snapshotAccountIdRef = useRef<string | null>(null);
  const pollBridgeRef = useRef<{
    setModalOpen?: (open: boolean) => void;
    setAfterSeq?: (seq: number) => void;
  }>({});

  stateRef.current = state;
  modalOpenRef.current = modalOpen;
  cloudStatusRef.current = cloudStatus;
  authAccountIdRef.current = authAccountId;

  useEffect(() => {
    if (state.status === 'ready' && supportAccountIdRef.current === authAccountIdRef.current) {
      snapshotRef.current = { conversation: state.conversation, messages: state.messages };
      snapshotAccountIdRef.current = authAccountIdRef.current;
    }
  }, [authAccountId, state]);

  // 并行拉取会话 + 最近消息（两次云端往返合并为 1 个 RTT）。首屏 20 条，更早的上滑分页。
  const fetchConversationSnapshot = useCallback(async () => {
    const [conversation, listed] = await Promise.all([
      supportChatApi.getConversation(),
      supportChatApi.listMessages({ limit: FIRST_SCREEN_MESSAGE_LIMIT }),
    ]);
    return {
      conversation,
      messages: mergeServerMessages([], listed.list),
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
      // Keep the cursor moving while notifications are disabled so re-enabling
      // the setting does not replay a stale backlog as new attention.
      if (configService.get('system.notificationEnabled') === false) return;
      for (const message of toNotify) {
        const attentionId = supportAttentionId(message.seq);
        void ipcBridge.notification.show
          .invoke({
            title: '客服回复了你',
            body: truncateNotificationBody(message.content),
            attention_id: attentionId,
            click_target: supportNotifyDeepLink(attentionId),
          })
          .catch(() => {
            // Permission denied or unsupported — badge remains the fallback.
          });
      }
    },
    [whoami?.userId]
  );

  const invalidateSupportSession = useCallback(() => {
    supportSessionGenerationRef.current += 1;
    supportOpenGenerationRef.current += 1;
    reportGenerationRef.current += 1;
    reportContextKeyRef.current = null;
    setConversationErrorReportContext(null);
    setReportModalInstanceKey((key) => key + 1);
    supportImagePreviewCache.clear();
  }, []);

  const createSupportSessionGuard = useCallback(() => {
    const expected = {
      generation: supportSessionGenerationRef.current,
      accountId: authAccountIdRef.current,
    };
    return () =>
      isSupportSessionCurrent(
        expected,
        {
          generation: supportSessionGenerationRef.current,
          accountId: authAccountIdRef.current,
        },
        cloudStatusRef.current
      );
  }, []);

  useEffect(() => {
    const previousAccountId = supportAccountIdRef.current;
    if (cloudStatus !== 'authenticated' || !authAccountId) {
      if (previousAccountId !== null) {
        invalidateSupportSession();
      }
      supportAccountIdRef.current = null;
      dispatch({ type: 'reset' });
      setModalOpen(false);
      snapshotRef.current = null;
      snapshotAccountIdRef.current = null;
      pollBridgeRef.current = {};
      return;
    }

    if (previousAccountId !== null && previousAccountId !== authAccountId) {
      invalidateSupportSession();
      dispatch({ type: 'reset' });
      setModalOpen(false);
      snapshotRef.current = null;
      snapshotAccountIdRef.current = null;
      pollBridgeRef.current = {};
    }
    supportAccountIdRef.current = authAccountId;

    const isCurrentSession = createSupportSessionGuard();

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
        if (!isCurrentSession()) return;
        dispatch({
          type: 'set-unread',
          unreadCount: Math.max(0, conversation.userUnreadCount),
        });
      },
      onMessages: (incoming) => {
        if (!isCurrentSession()) return;
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
                if (!isCurrentSession()) return;
                dispatch({ type: 'conversation-updated', conversation });
                dispatch({ type: 'set-unread', unreadCount: 0 });
                void ipcBridge.attention.clearScope
                  .invoke({ source: 'support' })
                  .catch(() => {
                    // Keep the native badge if the clear command cannot reach the shell.
                  });
              })
              .catch(() => {
                if (!isCurrentSession()) return;
                dispatch({ type: 'sync-warning', syncWarning: true });
              });
          }
        }
      },
      onError: (error) => {
        if (!isCurrentSession()) return;
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
    if (!snapshotRef.current || snapshotAccountIdRef.current !== authAccountId) {
      snapshotRef.current = null;
      snapshotAccountIdRef.current = null;
      void fetchConversationSnapshot()
        .then((snapshot) => {
          if (isCurrentSession() && !snapshotRef.current) {
            snapshotRef.current = snapshot;
            snapshotAccountIdRef.current = authAccountId;
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
  }, [
    authAccountId,
    cloudStatus,
    createSupportSessionGuard,
    fetchConversationSnapshot,
    invalidateSupportSession,
    notifyNewMessages,
  ]);

  useEffect(() => {
    pollBridgeRef.current.setModalOpen?.(modalOpen);
  }, [modalOpen]);

  useEffect(() => {
    if (state.status === 'ready') {
      pollBridgeRef.current.setAfterSeq?.(maxServerSeq(state.messages));
    }
  }, [state]);

  const openSupportChat = useCallback(() => {
    const openGeneration = ++supportOpenGenerationRef.current;
    if (cloudStatus !== 'authenticated') {
      dispatch({ type: 'auth-required' });
      setModalOpen(true);
      return;
    }
    const isCurrentSession = createSupportSessionGuard();
    setModalOpen(true);
    // 有快照（预热或上次打开的缓存）就直接渲染，刷新转后台；否则先展示骨架屏。
    const closedCache = stateRef.current.status === 'closed' ? stateRef.current.cached : undefined;
    const cached =
      closedCache ?? (snapshotAccountIdRef.current === authAccountId ? snapshotRef.current : null);
    if (cached) {
      dispatch({ type: 'ready', conversation: cached.conversation, messages: cached.messages });
    } else {
      dispatch({ type: 'open' });
    }
    void (async () => {
      const isCurrentOpen = () =>
        openGeneration === supportOpenGenerationRef.current && isCurrentSession();
      try {
        const snapshot = await fetchConversationSnapshot();
        if (!isCurrentOpen()) return;
        if (stateRef.current.status === 'ready') {
          // 已用缓存上屏：增量合并，保留发送中的 pending 气泡。
          dispatch({ type: 'conversation-updated', conversation: snapshot.conversation });
          dispatch({
            type: 'messages-merged',
            incoming: snapshot.messages
              .filter((item): item is Extract<SupportMessage, { kind: 'server' }> => item.kind === 'server')
              .map((item) => item.message),
          });
        } else {
          dispatch({ type: 'ready', conversation: snapshot.conversation, messages: snapshot.messages });
        }
        if (snapshot.conversation.userUnreadCount > 0 && snapshot.conversation.lastSeq > 0) {
          // 已读回执后台执行，不阻塞首屏。
          void supportChatApi
            .markRead(snapshot.conversation.lastSeq)
            .then((updated) => {
              if (!isCurrentOpen()) return;
              dispatch({ type: 'conversation-updated', conversation: updated });
              dispatch({ type: 'set-unread', unreadCount: 0 });
              void ipcBridge.attention.clearScope
                .invoke({ source: 'support' })
                .catch(() => {
                  // Keep the native badge if the clear command cannot reach the shell.
                });
            })
            .catch(() => {
              if (!isCurrentOpen()) return;
              dispatch({ type: 'sync-warning', syncWarning: true });
            });
        }
      } catch (error) {
        if (!isCurrentOpen()) return;
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
  }, [cloudStatus, createSupportSessionGuard, fetchConversationSnapshot]);

  const closeSupportChat = useCallback(() => {
    supportOpenGenerationRef.current += 1;
    setModalOpen(false);
    // Keep the reducer cache alive while in-flight sends finish. Closing the
    // surface must not turn a retryable pending message into an unobservable
    // request.
    dispatch({ type: 'close' });
  }, []);

  const sendWithClientMsgId = useCallback(
    async (
      clientMsgId: string,
      content: string,
      options: {
        msgType?: 'text' | 'image';
        payload?: ICloudImAttachmentPayload;
        logPayload?: ICloudImAttachmentPayload;
        /** Prevents queued work from crossing an auth boundary. */
        shouldContinue: () => boolean;
      }
    ): Promise<SupportSendOutcome> => {
      const task: Promise<SupportSendOutcome> = sendQueueRef.current.then(async () => {
        if (!options.shouldContinue()) {
          throw new Error('support operation cancelled');
        }
        const msgType = options.msgType ?? 'text';
        try {
          if (
            Array.from(content).length > MAX_SUPPORT_MESSAGE_CHARS ||
            (msgType === 'text' && content.trim().length === 0)
          ) {
            throw new Error('support message is outside the allowed content boundary');
          }
          const message = await supportChatApi.sendMessage({
            clientMsgId,
            content,
            msgType,
            payload: options.payload,
            logPayload: options.logPayload,
          });
          if (!options.shouldContinue()) return { accepted: true, applied: false };
          dispatch({ type: 'pending-replaced', clientMsgId, message });
          if (msgType === 'image' && !modalOpenRef.current) {
            supportImagePreviewCache.release(clientMsgId);
          }
          return { accepted: true, applied: true };
        } catch (error) {
          if (!options.shouldContinue()) {
            throw error;
          }
          dispatch({ type: 'pending-failed', clientMsgId });
          if (isAuthExpiredHttpError(error)) {
            dispatch({ type: 'auth-required' });
          }
          throw error;
        }
      });
      sendQueueRef.current = task.then(
        () => undefined,
        () => undefined
      );
      return task;
    },
    []
  );

  const sendMessage = useCallback(
    async (content: string, logPayload?: ICloudImAttachmentPayload): Promise<boolean> => {
      const trimmed = content.trim();
      const shouldContinue = createSupportSessionGuard();
      if (
        !trimmed ||
        Array.from(trimmed).length > MAX_SUPPORT_MESSAGE_CHARS ||
        !shouldContinue() ||
        stateRef.current.status !== 'ready'
      ) {
        return false;
      }
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
      const result = await sendWithClientMsgId(clientMsgId, trimmed, { logPayload, shouldContinue });
      // A stale operation may have been accepted by the server after this
      // session was invalidated. Keep the composer draft in that case because
      // the current surface never applied the result.
      return result.applied;
    },
    [createSupportSessionGuard, sendWithClientMsgId]
  );

  const reportConversationError = useCallback(
    (context: ConversationErrorReportContext) => {
      if (cloudStatus !== 'authenticated') {
        Message.warning(t('common.supportChat.authRequired'));
        openSupportChat();
        return;
      }
      setConversationErrorReportContext(context);
      const nextContextKey = getConversationErrorReportContextKey(context);
      if (reportContextKeyRef.current !== nextContextKey) {
        reportContextKeyRef.current = nextContextKey;
        reportGenerationRef.current += 1;
        setReportModalInstanceKey((key) => key + 1);
      }
    },
    [cloudStatus, openSupportChat, t]
  );

  const closeConversationErrorReport = useCallback(() => {
    reportGenerationRef.current += 1;
    setConversationErrorReportContext(null);
  }, []);

  const submitConversationErrorReport = useCallback(
    async (
      context: ConversationErrorReportContext,
      draft: ConversationErrorReportDraft
    ): Promise<ConversationErrorReportSubmitResult> => {
      const operationGeneration = reportGenerationRef.current;
      const operationAccountId = authAccountId;
      const isCurrentReportOperation = () =>
        reportGenerationRef.current === operationGeneration &&
        cloudStatusRef.current === 'authenticated' &&
        authAccountIdRef.current === operationAccountId;

      if (!isCurrentReportOperation()) return { status: 'preparation-failed' };

      const hasSupportChatState = () =>
        stateRef.current.status === 'ready' ||
        (stateRef.current.status === 'closed' && Boolean(stateRef.current.cached));
      if (!hasSupportChatState()) {
        try {
          // Reporting is intentionally available before the support window has
          // been opened. Hydrate the hidden conversation state first so the
          // existing pending/failed/retry flow can also represent a report
          // submitted from an error card.
          const snapshot = await fetchConversationSnapshot();
          if (!isCurrentReportOperation()) return { status: 'preparation-failed' };
          if (!hasSupportChatState()) {
            dispatch({ type: 'ready', conversation: snapshot.conversation, messages: snapshot.messages });
          }
        } catch (error) {
          if (isAuthExpiredHttpError(error) && isCurrentReportOperation()) {
            dispatch({ type: 'auth-required' });
          }
          return { status: 'preparation-failed' };
        }
      }

      return submitConversationErrorReportFlow(context, draft, {
        isCurrent: isCurrentReportOperation,
        packLogs: (params) => supportChatApi.packLogs(params),
        collectDevice: collectSupportDeviceInfo,
        uploadScreenshot: (params) => supportChatApi.uploadScreenshot(params),
        uploadLogFromPath: (params) => supportChatApi.uploadLogFromPath(params),
        account: collectSupportLogUserInfo(whoami),
        addPending: (message) => {
          if (message.msgType === 'image' && message.previewUrl) {
            supportImagePreviewCache.set(message.clientMsgId, message.previewUrl);
          }
          dispatch({ type: 'pending-added', message });
        },
        markPendingFailed: (clientMsgId) => {
          dispatch({ type: 'pending-failed', clientMsgId });
        },
        send: sendWithClientMsgId,
        onAuthExpired: () => {
          dispatch({ type: 'auth-required' });
        },
        defaultContent: t('settings.bugReportDefaultContent', {
          defaultValue: '提交了一个对话问题，请协助排查',
        }),
      });
    },
    [authAccountId, fetchConversationSnapshot, sendWithClientMsgId, t, whoami]
  );

  // 图片发送：同步挂出全部 pending 气泡（本地预览秒上屏），
  // 后台并行上传（文档 §4）、按顺序发送（文档 §5.2）；说明文字随最后一张，展示在整组图片之后。
  const sendImages = useCallback(
    (params: { content: string; images: SupportOutgoingImage[] }): boolean => {
      const shouldContinue = createSupportSessionGuard();
      const caption = params.content.trim();
      if (
        !shouldContinue() ||
        stateRef.current.status !== 'ready' ||
        params.images.length === 0 ||
        params.images.length > MAX_SUPPORT_IMAGES ||
        Array.from(caption).length > MAX_SUPPORT_MESSAGE_CHARS ||
        params.images.some(
          (image) =>
            !image.fileName.trim() ||
            image.file.size > MAX_SUPPORT_IMAGE_BYTES ||
            !getSupportImageContentType({ name: image.fileName, type: image.file.type })
        )
      ) {
        return false;
      }
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
          if (!shouldContinue()) {
            supportImagePreviewCache.release(entry.clientMsgId);
            return null;
          }
          try {
            const uploaded = await supportChatApi.uploadScreenshot({
              file: normalizeSupportImageFile(entry.file, entry.fileName),
              fileName: entry.fileName,
            });
            if (!shouldContinue()) {
              supportImagePreviewCache.release(entry.clientMsgId);
              return null;
            }
            return {
              entry,
              payload: buildSupportImagePayload(uploaded, {
                fileName: entry.fileName,
                contentType: entry.file.type,
                byteSize: entry.file.size,
              }),
            };
          } catch (error) {
            if (!shouldContinue()) {
              supportImagePreviewCache.release(entry.clientMsgId);
              return null;
            }
            dispatch({ type: 'pending-failed', clientMsgId: entry.clientMsgId });
            if (isAuthExpiredHttpError(error)) {
              dispatch({ type: 'auth-required' });
            }
            return null;
          }
        });
        for (const uploadTask of uploads) {
          const uploadedItem = await uploadTask;
          if (!shouldContinue()) {
            if (uploadedItem) supportImagePreviewCache.release(uploadedItem.entry.clientMsgId);
            return;
          }
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
            const sendResult = await sendWithClientMsgId(entry.clientMsgId, entry.content, {
              msgType: 'image',
              payload,
              shouldContinue,
            });
            if (!sendResult.applied) {
              supportImagePreviewCache.release(entry.clientMsgId);
            }
          } catch {
            // pending-failed 已在 sendWithClientMsgId 内标记，气泡上可重试。
          }
        }
      })();
      return true;
    },
    [createSupportSessionGuard, sendWithClientMsgId]
  );

  const retryMessage = useCallback(
    async (clientMsgId: string) => {
      const shouldContinue = createSupportSessionGuard();
      const current = stateRef.current;
      if (!shouldContinue() || current.status !== 'ready') return;
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
          if (shouldContinue()) dispatch({ type: 'pending-failed', clientMsgId });
          return;
        }
        try {
          if (!shouldContinue()) return;
          const uploaded = await supportChatApi.uploadScreenshot({
            file: normalizeSupportImageFile(pending.file, pending.fileName || 'screenshot.png'),
            fileName: pending.fileName || 'screenshot.png',
          });
          if (!shouldContinue()) return;
          payload = buildSupportImagePayload(uploaded, {
            fileName: pending.fileName || 'screenshot.png',
            contentType: pending.file.type,
            byteSize: pending.file.size,
          });
          dispatch({
            type: 'pending-added',
            message: { ...pending, delivery: 'sending', payload },
          });
        } catch (error) {
          if (!shouldContinue()) return;
          dispatch({ type: 'pending-failed', clientMsgId });
          if (isAuthExpiredHttpError(error)) {
            dispatch({ type: 'auth-required' });
          }
          return;
        }
      }
      if (!shouldContinue()) return;
      try {
        const sendResult = await sendWithClientMsgId(pending.clientMsgId, pending.content, {
          msgType: pending.msgType ?? 'text',
          payload,
          logPayload: pending.logPayload,
          shouldContinue,
        });
        if (!sendResult.applied && pending.msgType === 'image') {
          supportImagePreviewCache.release(pending.clientMsgId);
        }
      } catch {
        // pending-failed 已在 sendWithClientMsgId 内标记，气泡上可重试。
      }
    },
    [createSupportSessionGuard, sendWithClientMsgId]
  );

  const loadOlder = useCallback(async (): Promise<boolean> => {
    const shouldContinue = createSupportSessionGuard();
    const current = stateRef.current;
    if (!shouldContinue() || current.status !== 'ready' || loadingOlderRef.current) return false;
    const firstServer = current.messages.find((item) => item.kind === 'server');
    if (!firstServer || firstServer.kind !== 'server') return false;
    loadingOlderRef.current = true;
    try {
      const listed = await supportChatApi.listMessages({
        beforeSeq: firstServer.message.seq,
        limit: OLDER_PAGE_MESSAGE_LIMIT,
      });
      if (!shouldContinue()) return false;
      const existingSeqs = new Set(
        current.messages.flatMap((item) => (item.kind === 'server' ? [item.message.seq] : []))
      );
      const hasNewMessages = listed.list.some((message) => !existingSeqs.has(message.seq));
      if (hasNewMessages) {
        dispatch({ type: 'messages-merged', incoming: listed.list });
        return true;
      }
      return false;
    } catch (error) {
      if (!shouldContinue()) return false;
      if (isAuthExpiredHttpError(error)) {
        dispatch({ type: 'auth-required' });
      } else {
        dispatch({ type: 'sync-warning', syncWarning: true });
      }
      // Let the list distinguish a transient request failure from an empty
      // page. It must remain possible to retry by returning to the top.
      throw error;
    } finally {
      loadingOlderRef.current = false;
    }
  }, [createSupportSessionGuard]);

  const unreadCount = state.unreadCount;
  const hasUnread = unreadCount > 0;

  const value = useMemo<SupportChatContextValue>(
    () => ({
      openSupportChat,
      closeSupportChat,
      modalOpen,
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
      modalOpen,
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
      <ConversationErrorReportModal
        key={reportModalInstanceKey}
        context={conversationErrorReportContext}
        onCancel={closeConversationErrorReport}
        onSubmit={(draft) => {
          if (!conversationErrorReportContext) {
            return Promise.resolve({ status: 'preparation-failed' as const });
          }
          return submitConversationErrorReport(conversationErrorReportContext, draft);
        }}
        onOpenSupportChat={openSupportChat}
      />
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
