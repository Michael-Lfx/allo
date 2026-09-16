import type { ConversationId } from '@/common/types/ids';

import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@/common';
import { configService } from '@/common/config/configService';
import { getConversationOrNull } from '@/renderer/pages/conversation/utils/conversationCache';
import {
  conversationAttentionId,
  conversationNotifyDeepLink,
} from '@renderer/hooks/system/desktopNotifyDeepLink';
import { isConversationOnScreen } from '@renderer/hooks/system/conversationNotifyPolicy';
import { routePathnameFromHref } from '@renderer/utils/routes/sessionRoute';
import { clearConversationAttention } from '@renderer/utils/attention';

/**
 * Desktop OS notification when a conversation turn finishes.
 * Skips companion sessions, respects `system.notificationEnabled`, and does
 * not toast while the user is focused on that conversation. The decision is
 * re-evaluated after the async conversation fetch, and a suppressed
 * completion also clears that conversation's native attention so a stale
 * badge can never survive a false-negative focus reading.
 */
export const useConversationDesktopNotify = () => {
  const { t } = useTranslation();

  useEffect(() => {
    // Local inputs first: most completions are not for a conversation the user
    // is looking at, and only the app-focus input needs a native round-trip.
    const isViewingConversationNow = async (conversationId: ConversationId): Promise<boolean> => {
      const visible = typeof document === 'undefined' || document.visibilityState === 'visible';
      const pathname =
        typeof window === 'undefined' ? '' : routePathnameFromHref(window.location.href);
      if (!isConversationOnScreen({ visible, pathname, conversationId })) return false;
      return ipcBridge.windowControls.isAppFocused.invoke();
    };

    return ipcBridge.conversation.turnCompleted.on((event) => {
      if (event.status !== 'finished') return;
      if (configService.get('system.notificationEnabled') === false) return;

      const conversationId = event.conversation_id;

      void (async () => {
        if (await isViewingConversationNow(conversationId)) {
          // The user is already looking at this conversation on a focused app,
          // so this attention is handled: clear the scope instead of notifying.
          await clearConversationAttention(conversationId);
          return;
        }

        const conversation = await getConversationOrNull(conversationId);
        if (!conversation) return;
        if (conversation.type === 'nomi' && conversation.extra.companion_session) return;

        // The conversation fetch is async: the user may have switched into the
        // conversation while it was in flight. Never toast what is now being
        // read on screen.
        if (await isViewingConversationNow(conversationId)) {
          await clearConversationAttention(conversationId);
          return;
        }

        const body =
          event.state === 'error'
            ? t('conversation.notify.failedBody', { defaultValue: '失败' })
            : event.state === 'stopped'
              ? t('conversation.notify.stoppedBody', { defaultValue: '已停止' })
              : t('conversation.notify.doneBody', { defaultValue: '已完成' });

        const title =
          conversation.name?.trim() ||
          t('conversation.notify.fallbackTitle', { defaultValue: '对话' });
        if (event.turn_id == null) {
          console.warn(
            '[DesktopNotification] conversation completion is missing turn_id; using conversation latest attention'
          );
        }
        const attentionId = conversationAttentionId(
          String(conversationId),
          event.turn_id == null ? undefined : String(event.turn_id)
        );

        await ipcBridge.notification.show
          .invoke({
            title,
            body,
            conversation_id: conversationId as ConversationId,
            attention_id: attentionId,
            click_target: conversationNotifyDeepLink(String(conversationId), attentionId),
          })
          .catch(() => {
            /* permission / unsupported host */
          });
      })();
    });
  }, [t]);
};
