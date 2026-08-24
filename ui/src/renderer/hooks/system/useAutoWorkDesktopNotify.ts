import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@/common';
import { isTauriRuntime } from '@/common/adapter/tauriRuntime';
import { configService } from '@/common/config/configService';
import { requirementNotifyDeepLink } from '@renderer/hooks/system/desktopNotifyDeepLink';

/**
 * Web / non-desktop OS notifications for requirement terminal status.
 * Desktop uses backend `SystemCompletionNotifier` + `DesktopTauriNotifier`
 * instead (avoids duplicate toasts).
 */
export const useAutoWorkDesktopNotify = () => {
  const { t } = useTranslation();

  useEffect(() => {
    if (isTauriRuntime()) return;
    return ipcBridge.requirements.onStatusChanged.on((req) => {
      if (req.status !== 'done' && req.status !== 'failed' && req.status !== 'needs_review') {
        return;
      }
      if (configService.get('system.notificationEnabled') === false) return;

      const title =
        req.status === 'done'
          ? t('requirements.notify.doneTitle', { defaultValue: 'Requirement completed' })
          : req.status === 'failed'
            ? t('requirements.notify.failedTitle', { defaultValue: 'Requirement failed' })
            : t('requirements.notify.needsReviewTitle', { defaultValue: 'Requirement needs review' });
      const body = req.title
        ? `${req.tag ? `[${req.tag}] ` : ''}${req.title}`
        : String(req.requirement_id);
      void ipcBridge.notification.show
        .invoke({
          title,
          body,
          click_target: requirementNotifyDeepLink(req.tag, String(req.requirement_id)),
        })
        .catch(() => {
          /* notification permission / unsupported host — ignore */
        });
    });
  }, [t]);
};
