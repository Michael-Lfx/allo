import { ipcBridge } from '@/common';
import { AppMessage as Message } from '@/renderer/components/notifications';
import React, { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate, useParams } from 'react-router-dom';
import useSWR from 'swr';
import ChatConversation from './components/ChatConversation';
import MessageListSkeleton from './Messages/components/MessageListSkeleton';
import { getConversationOrNull } from '@/renderer/pages/conversation/utils/conversationCache';
import { parseConversationId } from '@/common/types/ids';
import { emitter } from '@/renderer/utils/emitter';

const ChatConversationIndex: React.FC = () => {
  const { id } = useParams();
  // Validate the route string once at the boundary; every downstream layer
  // keeps the same canonical conversation entity ID.
  const conversationId = id != null ? parseConversationId(id) : undefined;
  const { t } = useTranslation();
  const location = useLocation();
  const navigate = useNavigate();
  const notFoundHandledIdRef = useRef<string | undefined>(undefined);
  const deletedHandledIdRef = useRef<string | undefined>(undefined);
  const clearedAttentionKeyRef = useRef<string | undefined>(undefined);

  const { data, isLoading, mutate } = useSWR(id ? `conversation/${id}` : null, () => {
    return getConversationOrNull(conversationId!);
  });

  useEffect(() => {
    if (!conversationId || isLoading || !data) return;
    const clearCurrentConversationAttention = () => {
      const requestedAttentionId = new URLSearchParams(location.search).get('attention_id');
      const exactAttentionId = requestedAttentionId?.startsWith(`conversation:${conversationId}:`)
        ? requestedAttentionId
        : undefined;
      // A supplied but foreign/malformed attention id must never fall back to
      // a conversation-wide clear: another turn may still need attention.
      if (requestedAttentionId !== null && !exactAttentionId) return;
      const clearKey = exactAttentionId ?? `conversation-scope:${conversationId}`;
      if (clearedAttentionKeyRef.current === clearKey) return;
      clearedAttentionKeyRef.current = clearKey;
      void (exactAttentionId
        ? ipcBridge.attention.clear.invoke({ attention_id: exactAttentionId })
        : ipcBridge.attention.clearScope.invoke({
            source: 'conversation',
            entity_id: String(conversationId),
          })
      ).catch(() => {
        // Keep native attention if the renderer cannot confirm the page load.
        if (clearedAttentionKeyRef.current === clearKey) {
          clearedAttentionKeyRef.current = undefined;
        }
      });
    };

    // If a completion arrived while another app had focus, the page was
    // already mounted and the initial load effect has nothing new to observe.
    // Retry the precise conversation clear when the user returns to this page.
    clearCurrentConversationAttention();
    const onWindowFocus = () => clearCurrentConversationAttention();
    const onVisibilityChange = () => {
      if (document.visibilityState === 'visible') clearCurrentConversationAttention();
    };
    window.addEventListener('focus', onWindowFocus);
    document.addEventListener('visibilitychange', onVisibilityChange);
    return () => {
      window.removeEventListener('focus', onWindowFocus);
      document.removeEventListener('visibilitychange', onVisibilityChange);
    };
  }, [conversationId, data, isLoading, location.search]);

  useEffect(() => {
    if (!id) return;

    return ipcBridge.conversation.listChanged.on((event) => {
      if (event.conversation_id !== conversationId) {
        return;
      }

      if (event.action === 'deleted') {
        if (deletedHandledIdRef.current === id) {
          return;
        }
        deletedHandledIdRef.current = id;

        // The backend publishes `deleted` only after the persistent row is
        // authoritatively gone. This is also the success path for a delete
        // request whose HTTP waiter timed out while its detached coordinator
        // kept working. Reuse the local deletion channel so command queues and
        // other per-conversation state are cleared before the route unmounts.
        emitter.emit('conversation.deleted', conversationId);
        void mutate(undefined, { revalidate: false });
        void navigate('/guid', { replace: true });
        return;
      }

      if (event.action !== 'updated' && event.action !== 'created') {
        return;
      }

      void mutate();
    });
  }, [id, conversationId, mutate, navigate]);

  // 会话不存在（例如从历史栈回到已删除会话）时，提示并替换路由到首页，
  // 避免渲染空骨架。每个 id 只触发一次。
  // Conversation does not exist (e.g. navigating back to a deleted one via
  // browser history): show a toast and replace the route with home, so we
  // don't render an empty skeleton. Fire at most once per id.
  useEffect(() => {
    if (!id || isLoading || data || notFoundHandledIdRef.current === id) return;
    notFoundHandledIdRef.current = id;
    Message.warning(t('conversation.notFound'));
    navigate('/', { replace: true });
  }, [id, isLoading, data, navigate, t]);

  if (isLoading) return <MessageListSkeleton />;
  return <ChatConversation conversation={data ?? undefined}></ChatConversation>;
};

export default ChatConversationIndex;
