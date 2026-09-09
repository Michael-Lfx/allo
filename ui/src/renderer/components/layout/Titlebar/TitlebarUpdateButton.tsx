import React, { useCallback } from 'react';
import classNames from 'classnames';
import { Download } from '@icon-park/react';
import { useTranslation } from 'react-i18next';

import InstantHoverTooltip from '@renderer/components/base/InstantHoverTooltip';
import { useUpdateAvailability } from '@renderer/hooks/system/useUpdateAvailability';
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
 * Consumes the shared availability store (filled by Layout's deferred startup
 * check / UpdateModal). Does not hit ModelScope itself — that duplicate mount
 * check used to race the first paint.
 */
const TitlebarUpdateButton: React.FC<TitlebarUpdateButtonProps> = ({ iconSize, strokeWidth, className }) => {
  const { t } = useTranslation();
  const availability = useUpdateAvailability();
  const hasUpdate = availability.available;
  const latestVersion = availability.version ?? null;

  const openUpdateModal = useCallback(() => {
    window.dispatchEvent(new CustomEvent('nomifun-open-update-modal', { detail: { source: 'titlebar' } }));
  }, []);

  if (!isDesktopShell() || !hasUpdate) return null;

  const tooltip = t('update.titlebarUpdateAvailable', { version: latestVersion ?? '' });

  return (
    <InstantHoverTooltip
      content={tooltip}
      position='bottom'
      hoverDelayMs={400}
      className='app-titlebar__tooltip-anchor'
      dataTauriNoDrag
    >
      <button
        type='button'
        className={classNames('app-titlebar__button relative', className)}
        onClick={openUpdateModal}
        aria-label={tooltip}
        data-tauri-no-drag
      >
        <Download theme='outline' size={iconSize} fill='currentColor' strokeWidth={strokeWidth} />
        <UpdateBadge />
      </button>
    </InstantHoverTooltip>
  );
};

export default TitlebarUpdateButton;
