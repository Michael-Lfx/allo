
import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@/common';
import { isDesktopShell } from '@/renderer/utils/platform';

/**
 * Sync native system-tray menu labels (Show / Quit) to the current UI locale.
 * Meeting controls are intentionally not part of the tray menu.
 * No-op outside the Tauri desktop shell.
 */
export function useTrayLabels(): void {
  const { t, i18n } = useTranslation();
  useEffect(() => {
    if (!isDesktopShell()) return;
    void ipcBridge.application.setTrayLabels
      .invoke({
        show: t('common.tray.showWindow'),
        quit: t('common.tray.quit'),
      })
      .catch(() => {});
  }, [t, i18n.language]);
}
