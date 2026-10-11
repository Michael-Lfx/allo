import React from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@arco-design/web-react';
import { Shop } from '@icon-park/react';
import classNames from 'classnames';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';
import {
  APP_CHROME_SHORTCUT_BADGE_CLASS,
  appChromeShortcutTooltip,
  formatAppChromeShortcut,
  isAppChromeShortcutAvailable,
} from '@/renderer/utils/appChromeShortcuts';
import { isDesktopShell } from '@renderer/utils/platform';

interface SiderModelStoreEntryProps {
  isMobile: boolean;
  isActive: boolean;
  collapsed: boolean;
  siderTooltipProps: SiderTooltipProps;
  onClick: () => void;
}

/**
 * Model management — the top action-area entry sitting directly below the
 * search button. Aligned with the new-conversation / search chrome.
 */
const SiderModelStoreEntry: React.FC<SiderModelStoreEntryProps> = ({
  isMobile,
  isActive,
  collapsed,
  siderTooltipProps,
  onClick,
}) => {
  const { t } = useTranslation();
  const label = t('modelStore.railTitle');
  const shortcutAvailable = isAppChromeShortcutAvailable('modelStore', {
    desktop: isDesktopShell(),
    mobile: isMobile,
  });
  const tooltip = shortcutAvailable ? appChromeShortcutTooltip(label, 'modelStore') : label;

  if (collapsed) {
    return (
      <Tooltip {...siderTooltipProps} content={tooltip} position='right'>
        <button
          type='button'
          data-testid='sider-model-store-entry'
          className={classNames(
            'w-full h-34px flex items-center justify-center cursor-pointer shrink-0 transition-colors rd-8px border-none p-0 focus:outline-none',
            isActive ? 'bg-primary-1 text-primary-6' : 'bg-transparent text-t-primary hover:bg-fill-3 active:bg-fill-4'
          )}
          onClick={onClick}
          aria-label={tooltip}
          aria-current={isActive ? 'page' : undefined}
        >
          <Shop
            theme='outline'
            size='16'
            fill='currentColor'
            className='block leading-none'
            style={{ lineHeight: 0 }}
          />
        </button>
      </Tooltip>
    );
  }

  return (
    <Tooltip {...siderTooltipProps} content={tooltip} position='right'>
      <button
        type='button'
        data-testid='sider-model-store-entry'
        className={classNames(
          'h-34px w-full flex items-center justify-start gap-8px pl-10px pr-8px shrink-0 rd-0.5rem bg-transparent border-none cursor-pointer transition-colors focus:outline-none focus-visible:outline-none select-none',
          isMobile && 'sider-action-btn-mobile',
          isActive
            ? 'bg-primary-1 text-primary-6'
            : 'text-t-primary hover:bg-fill-3 active:bg-fill-4'
        )}
        onClick={onClick}
        aria-label={tooltip}
        aria-current={isActive ? 'page' : undefined}
      >
        <span className='size-22px flex items-center justify-center shrink-0'>
          <Shop
            theme='outline'
            size='16'
            fill='currentColor'
            className='block leading-none shrink-0'
            style={{ lineHeight: 0 }}
          />
        </span>
        <span className='collapsed-hidden text-14px font-[500] leading-24px truncate'>{label}</span>
        {shortcutAvailable ? (
          <span className={APP_CHROME_SHORTCUT_BADGE_CLASS}>{formatAppChromeShortcut('modelStore')}</span>
        ) : null}
      </button>
    </Tooltip>
  );
};

export default SiderModelStoreEntry;
