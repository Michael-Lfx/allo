/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Popover, Tooltip } from '@arco-design/web-react';
import { Logout, Right, Theme, User } from '@icon-park/react';
import classNames from 'classnames';
import { ipcBridge } from '@/common';
import type { IMediaCredits } from '@/common/adapter/ipcBridge';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';
import SiderThemePanel from './SiderThemePanel';

interface SiderUserMenuProps {
  isMobile: boolean;
  collapsed: boolean;
  siderTooltipProps: SiderTooltipProps;
  userLabel?: string;
  planLabel?: string;
  showLogout?: boolean;
  onLogout?: () => void;
}

const menuRowClass =
  'flex items-center gap-8px w-full h-30px px-8px rd-6px text-left border-none bg-transparent cursor-pointer transition-colors hover:bg-fill-2 active:bg-fill-3';

const SiderUserMenu: React.FC<SiderUserMenuProps> = ({
  isMobile,
  collapsed,
  siderTooltipProps,
  userLabel,
  planLabel,
  showLogout = false,
  onLogout,
}) => {
  const { t } = useTranslation();
  const displayName = userLabel?.trim() || '—';
  const planText = planLabel?.trim() || '';
  const [menuVisible, setMenuVisible] = useState(false);
  const [skinVisible, setSkinVisible] = useState(false);
  const [credits, setCredits] = useState<IMediaCredits | null>(null);
  const [creditsLoading, setCreditsLoading] = useState(false);

  const refreshCredits = useCallback(async () => {
    setCreditsLoading(true);
    try {
      const result = await ipcBridge.media.getCredits.invoke();
      setCredits(result);
    } catch {
      setCredits(null);
    } finally {
      setCreditsLoading(false);
    }
  }, []);

  const handleMenuVisibleChange = (visible: boolean) => {
    setMenuVisible(visible);
    if (!visible) {
      setSkinVisible(false);
      return;
    }
    void refreshCredits();
  };

  const handleLogout = () => {
    setMenuVisible(false);
    setSkinVisible(false);
    onLogout?.();
  };

  const creditsText = creditsLoading
    ? t('common.userMenu.loadingCredits', { defaultValue: '加载中…' })
    : credits != null
      ? String(credits.balance)
      : t('common.userMenu.creditsUnavailable', { defaultValue: '—' });

  const menuContent = (
    <div className='w-192px flex flex-col gap-1px p-4px'>
      <div className='flex items-center justify-between gap-8px h-30px px-8px text-12px'>
        <span className='text-t-secondary'>{t('common.userMenu.creditsBalance', { defaultValue: '积分余额' })}</span>
        <span className='font-600 text-t-primary tabular-nums'>{creditsText}</span>
      </div>

      <Popover
        className='sider-soft-popover sider-user-skin-popover'
        trigger='click'
        position='rt'
        popupVisible={skinVisible}
        onVisibleChange={setSkinVisible}
        getPopupContainer={() => document.body}
        content={
          <SiderThemePanel
            className='w-280px'
            onBeforeOpenModal={() => {
              setSkinVisible(false);
              setMenuVisible(false);
            }}
          />
        }
        unmountOnExit={false}
      >
        <button type='button' className={classNames(menuRowClass, skinVisible && '!bg-fill-2')}>
          <Theme theme='outline' size='14' fill='currentColor' className='shrink-0 text-t-secondary' />
          <span className='flex-1 text-12px text-t-primary'>{t('common.userMenu.changeSkin', { defaultValue: '换肤' })}</span>
          <Right theme='outline' size='12' fill='currentColor' className='shrink-0 text-t-tertiary' />
        </button>
      </Popover>

      {showLogout && onLogout && (
        <>
          <div className='mx-4px h-1px bg-[var(--color-border-2)]' />
          <button type='button' className={menuRowClass} onClick={handleLogout}>
            <Logout theme='outline' size='14' fill='currentColor' className='shrink-0 text-t-secondary' />
            <span className='flex-1 text-12px text-t-primary'>
              {t('common.userMenu.logout', { defaultValue: '退出登录' })}
            </span>
          </button>
        </>
      )}
    </div>
  );

  const trigger = (
    <div
      role='button'
      tabIndex={0}
      onKeyDown={(event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          handleMenuVisibleChange(!menuVisible);
        }
      }}
      className={classNames(
        'flex items-center min-w-0 transition-colors rd-0.5rem cursor-pointer hover:bg-fill-2 active:bg-fill-3',
        collapsed ? 'h-34px w-full justify-center px-0' : 'h-40px flex-1 justify-start gap-8px pl-8px pr-4px',
        isMobile && 'sider-footer-btn-mobile',
        menuVisible && '!bg-fill-2'
      )}
    >
      <span
        className={classNames(
          'flex items-center justify-center shrink-0 text-t-secondary bg-fill-2',
          collapsed ? 'size-22px rd-6px' : 'size-28px rd-full'
        )}
      >
        <User
          theme='outline'
          size={collapsed ? '16' : '15'}
          fill='currentColor'
          className='block leading-none'
          style={{ lineHeight: 0 }}
        />
      </span>
      {!collapsed && (
        <span className='min-w-0 flex-1 flex flex-col justify-center gap-1px'>
          <span className='truncate text-12px font-500 leading-16px text-t-primary'>{displayName}</span>
          {planText ? (
            <span className='truncate text-11px leading-14px text-t-tertiary'>{planText}</span>
          ) : null}
        </span>
      )}
    </div>
  );

  return (
    <Popover
      className='sider-soft-popover sider-user-menu-popover'
      trigger='click'
      position={collapsed ? 'rt' : 'tr'}
      popupVisible={menuVisible}
      onVisibleChange={handleMenuVisibleChange}
      getPopupContainer={() => document.body}
      content={menuContent}
      unmountOnExit={false}
      {...({
        popupAlign: collapsed ? { left: 10 } : { bottom: 8, left: -28 },
      } as Record<string, unknown>)}
    >
      <Tooltip
        {...siderTooltipProps}
        content={planText ? `${displayName} · ${planText}` : displayName}
        position='right'
      >
        {trigger}
      </Tooltip>
    </Popover>
  );
};

export default SiderUserMenu;
