
import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@/common';
import { isDesktopShell } from '@/renderer/utils/platform';

/**
 * Sync native system-tray menu labels (Show / Quit / meeting controls) to the
 * current UI locale. No-op outside the Tauri desktop shell.
 */
export function useTrayLabels(): void {
  const { t, i18n } = useTranslation();
  useEffect(() => {
    if (!isDesktopShell()) return;
    void ipcBridge.application.setTrayLabels
      .invoke({
        show: t('common.tray.showWindow'),
        quit: t('common.tray.quit'),
        start: t('common.tray.meetingStart'),
        pause: t('common.tray.meetingPause'),
        resume: t('common.tray.meetingResume'),
        stop: t('common.tray.meetingStop'),
        open: t('common.tray.meetingOpen'),
      })
      .catch(() => {});
  }, [t, i18n.language]);
}
