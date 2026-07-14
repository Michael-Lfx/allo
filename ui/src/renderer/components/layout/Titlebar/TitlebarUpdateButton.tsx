import React, { useCallback, useEffect, useRef, useState } from 'react';
import classNames from 'classnames';
import { Download } from '@icon-park/react';
import { useTranslation } from 'react-i18next';

import { ipcBridge } from '@/common';
import InstantHoverTooltip from '@renderer/components/base/InstantHoverTooltip';
import { isDesktopShell } from '@/renderer/utils/platform';

/** Custom event Layout dispatches when a startup check finds an update. */
export const UPDATE_AVAILABLE_EVENT = 'nomifun-update-available';

export interface UpdateAvailableDetail {
  version: string;
}

interface TitlebarUpdateButtonProps {
  iconSize: number;
  strokeWidth?: number;
  className?: string;
}

/** Small badge dot shown when a newer signed release is available. */
const UpdateBadge: React.FC = () => (
  <span
    className='absolute rounded-full bg-red-500 ring-2 ring-[var(--color-bg-2)]'
    style={{ width: 7, height: 7, top: -1, right: -1 }}
    aria-hidden='true'
  />
);

/**
 * Desktop-only titlebar entry for in-app updates.
 *
 * - Silent check on mount; the icon is shown only when a newer version exists.
 * - Listens for startup check results from Layout.
 * - Click opens UpdateModal via the existing custom event.
 */
const TitlebarUpdateButton: React.FC<TitlebarUpdateButtonProps> = ({ iconSize, strokeWidth, className }) => {
  const { t } = useTranslation();
  const [hasUpdate, setHasUpdate] = useState(false);
  const [latestVersion, setLatestVersion] = useState<string | null>(null);
  const checkedRef = useRef(false);

  const runSilentCheck = useCallback(async () => {
    if (!isDesktopShell()) return;
    const includePrerelease = localStorage.getItem('update.includePrerelease') === 'true';
    try {
      const res = await ipcBridge.autoUpdate.check.invoke({ includePrerelease });
      if (res?.success && res.data?.updateInfo?.version) {
        setHasUpdate(true);
        setLatestVersion(res.data.updateInfo.version);
        window.dispatchEvent(
          new CustomEvent<UpdateAvailableDetail>(UPDATE_AVAILABLE_EVENT, {
            detail: { version: res.data.updateInfo.version },
          }),
        );
      } else {
        setHasUpdate(false);
        setLatestVersion(null);
      }
    } catch {
      /* offline / endpoint unreachable — hide icon until a check succeeds */
      setHasUpdate(false);
      setLatestVersion(null);
    }
  }, []);

  useEffect(() => {
    if (!isDesktopShell() || checkedRef.current) return;
    checkedRef.current = true;
    void runSilentCheck();
  }, [runSilentCheck]);

  useEffect(() => {
    if (!isDesktopShell()) return undefined;
    const onAvailable = (event: Event) => {
      const detail = (event as CustomEvent<UpdateAvailableDetail>).detail;
      if (detail?.version) {
        setHasUpdate(true);
        setLatestVersion(detail.version);
      }
    };
    window.addEventListener(UPDATE_AVAILABLE_EVENT, onAvailable as EventListener);
    return () => window.removeEventListener(UPDATE_AVAILABLE_EVENT, onAvailable as EventListener);
  }, []);

  const openUpdateModal = useCallback(() => {
    window.dispatchEvent(new CustomEvent('nomifun-open-update-modal', { detail: { source: 'titlebar' } }));
  }, []);

  if (!isDesktopShell() || !hasUpdate) return null;

  const tooltip = t('update.titlebarUpdateAvailable', { version: latestVersion ?? '' });

  return (
    <InstantHoverTooltip content={tooltip} position='bottom'>
      <button
        type='button'
        className={classNames('app-titlebar__button relative', className)}
        onClick={openUpdateModal}
        aria-label={tooltip}
      >
        <Download theme='outline' size={iconSize} fill='currentColor' strokeWidth={strokeWidth} />
        <UpdateBadge />
      </button>
    </InstantHoverTooltip>
  );
};

export default TitlebarUpdateButton;
