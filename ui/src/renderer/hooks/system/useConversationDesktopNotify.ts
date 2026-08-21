import type { ConversationId } from '@/common/types/ids';

import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'react-router-dom';
import { ipcBridge } from '@/common';
import { configService } from '@/common/config/configService';
import { getConversationOrNull } from '@/renderer/pages/conversation/utils/conversationCache';
import { conversationNotifyDeepLink } from '@renderer/hooks/system/desktopNotifyDeepLink';

/**
 * Desktop OS notification when a conversation turn finishes.
 * Skips companion sessions, respects `system.notificationEnabled`, and does
 * not toast when the user is already focused on that conversation.
 */
export const useConversationDesktopNotify = () => {
  const { t } = useTranslation();
  const location = useLocation();

  useEffect(() => {
    return ipcBridge.conversation.turnCompleted.on((event) => {
      if (event.status !== 'finished') return;
      if (configService.get('system.notificationEnabled') === false) return;

      const conversationId = event.conversation_id;
      const viewingSame =
        typeof document !== 'undefined' &&
        document.visibilityState === 'visible' &&
        document.hasFocus() &&
        location.pathname === `/conversation/${conversationId}`;
      if (viewingSame) return;

      void (async () => {
        const conversation = await getConversationOrNull(conversationId);
        if (!conversation) return;
        if (conversation.type === 'nomi' && conversation.extra.companion_session) return;

        const body =
          event.state === 'error'
            ? t('conversation.notify.failedBody', { defaultValue: '失败' })
            : event.state === 'stopped'
              ? t('conversation.notify.stoppedBody', { defaultValue: '已停止' })
              : t('conversation.notify.doneBody', { defaultValue: '已完成' });

        const title =
          conversation.name?.trim() ||
          t('conversation.notify.fallbackTitle', { defaultValue: '对话' });

        await ipcBridge.notification.show
          .invoke({
            title,
            body,
            conversation_id: conversationId as ConversationId,
            click_target: conversationNotifyDeepLink(String(conversationId)),
          })
          .catch(() => {
            /* permission / unsupported host */
          });
      })();
    });
  }, [location.pathname, t]);
};
