/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Popover, Tooltip } from '@arco-design/web-react';
import { Check, Logout, Message, Peoples, Right, Theme, Translate, User } from '@icon-park/react';
import classNames from 'classnames';
import { useCredits } from '@/renderer/hooks/context/CreditsContext';
import CreditsWebsiteButton from '@/renderer/components/base/CreditsWebsiteButton';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';
import { useSupportChat } from '@/renderer/features/supportChat/SupportChatProvider';
import { changeLanguage, normalizeLanguageCode, supportedLanguages } from '@/renderer/services/i18n';
import SiderThemePanel from './SiderThemePanel';

const LANGUAGE_LABELS: Record<string, string> = {
  'zh-CN': '简体中文',
  'en-US': 'English',
};

interface SiderUserMenuProps {
  isMobile: boolean;
  collapsed: boolean;
  siderTooltipProps: SiderTooltipProps;
  userLabel?: string;
  planLabel?: string;
  showLogout?: boolean;
  onLogout?: () => void;
  onOpenCompanion?: () => void;
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
  onOpenCompanion,
}) => {
  const { t, i18n } = useTranslation();
  const { openSupportChat, hasUnread, unreadCount } = useSupportChat();
  const displayName = userLabel?.trim() || '—';
  const planText = planLabel?.trim() || '';
  const [menuVisible, setMenuVisible] = useState(false);
  const [skinVisible, setSkinVisible] = useState(false);
  const [languageVisible, setLanguageVisible] = useState(false);
  const { balance, authenticated, isFetchingBalance, lastRefreshAt } = useCredits();

  const handleMenuVisibleChange = (visible: boolean) => {
    setMenuVisible(visible);
    if (!visible) {
      setSkinVisible(false);
      setLanguageVisible(false);
    }
  };

  const handleLogout = () => {
    setMenuVisible(false);
    setSkinVisible(false);
    setLanguageVisible(false);
    onLogout?.();
  };

  const handleOpenSupportChat = () => {
    setMenuVisible(false);
    setSkinVisible(false);
    setLanguageVisible(false);
    openSupportChat();
  };

  const handleOpenCompanion = () => {
    setMenuVisible(false);
    setSkinVisible(false);
    setLanguageVisible(false);
    onOpenCompanion?.();
  };

  const currentLanguage = normalizeLanguageCode(i18n.language);

  const handleLanguageSelect = (language: string) => {
    const normalizedLanguage = normalizeLanguageCode(language);
    setLanguageVisible(false);
    setMenuVisible(false);
    if (normalizedLanguage === currentLanguage) return;

    const apply = () => {
      changeLanguage(normalizedLanguage).catch((error: Error) => {
        console.error('Failed to change language:', error);
      });
    };

    if (typeof window !== 'undefined' && 'requestAnimationFrame' in window) {
      window.requestAnimationFrame(() => window.requestAnimationFrame(apply));
    } else {
      apply();
    }
  };

  const creditsText = !authenticated
    ? t('common.userMenu.creditsUnavailable', { defaultValue: '—' })
    : isFetchingBalance && lastRefreshAt === 0
      ? t('common.userMenu.loadingCredits', { defaultValue: '加载中…' })
      : String(balance);

  const unreadBadge = unreadCount > 99 ? '99+' : String(unreadCount);
  const supportLabel = hasUnread
    ? t('common.userMenu.contactSupportWithUnread', {
        defaultValue: '联系客服，{{count}} 条未读回复',
        count: unreadCount,
      })
    : t('common.userMenu.contactSupport', { defaultValue: '联系客服' });

  const menuContent = (
    <div className='w-192px flex flex-col gap-1px p-4px'>
      <div className='flex items-center justify-between gap-8px h-30px px-8px text-12px'>
        <span className='text-t-secondary'>{t('common.userMenu.creditsBalance', { defaultValue: '积分余额' })}</span>
        <span className='flex items-center gap-4px'>
          <span className='font-600 text-t-primary tabular-nums'>{creditsText}</span>
          {authenticated && (
            <CreditsWebsiteButton size='xs' />
          )}
        </span>
      </div>

      {onOpenCompanion && (
        <button type='button' className={menuRowClass} onClick={handleOpenCompanion}>
          <Peoples theme='outline' size='14' fill='currentColor' className='shrink-0 text-t-secondary' />
          <span className='flex-1 text-12px text-t-primary'>{t('nomi.siderTitle')}</span>
        </button>
      )}

      <Popover
        className='sider-soft-popover sider-user-language-popover'
        trigger='click'
        position='rt'
        popupVisible={languageVisible}
        onVisibleChange={(visible) => {
          setLanguageVisible(visible);
          if (visible) setSkinVisible(false);
        }}
        getPopupContainer={() => document.body}
        content={
          <div className='w-152px flex flex-col gap-1px p-4px'>
            {supportedLanguages.map((language) => {
              const normalizedLanguage = normalizeLanguageCode(language);
              const active = normalizedLanguage === currentLanguage;
              return (
                <button
                  type='button'
                  key={language}
                  className={menuRowClass}
                  aria-current={active ? 'true' : undefined}
                  onClick={() => handleLanguageSelect(language)}
                >
                  <span className='flex-1 text-12px text-t-primary'>
                    {LANGUAGE_LABELS[language] ?? language}
                  </span>
                  {active ? (
                    <Check theme='outline' size='14' fill='currentColor' className='shrink-0 text-primary-6' />
                  ) : null}
                </button>
              );
            })}
          </div>
        }
        unmountOnExit={false}
      >
        <button type='button' className={classNames(menuRowClass, languageVisible && '!bg-fill-2')}>
          <Translate theme='outline' size='14' fill='currentColor' className='shrink-0 text-t-secondary' />
          <span className='flex-1 text-12px text-t-primary'>
            {t('common.userMenu.language', { defaultValue: '语言' })}
          </span>
          <span className='text-11px text-t-tertiary'>{LANGUAGE_LABELS[currentLanguage] ?? currentLanguage}</span>
          <Right theme='outline' size='12' fill='currentColor' className='shrink-0 text-t-tertiary' />
        </button>
      </Popover>

      <Popover
        className='sider-soft-popover sider-user-skin-popover'
        trigger='click'
        position='rt'
        popupVisible={skinVisible}
        onVisibleChange={(visible) => {
          setSkinVisible(visible);
          if (visible) setLanguageVisible(false);
        }}
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

      <button
        type='button'
        className={menuRowClass}
        aria-label={supportLabel}
        onClick={handleOpenSupportChat}
      >
        <Message theme='outline' size='14' fill='currentColor' className='shrink-0 text-t-secondary' />
        <span className='flex-1 text-12px text-t-primary'>
          {t('common.userMenu.contactSupport', { defaultValue: '联系客服' })}
        </span>
        {hasUnread ? (
          <span className='min-w-16px h-16px px-4px rd-full bg-danger text-white text-10px leading-16px text-center tabular-nums'>
            {unreadBadge}
          </span>
        ) : null}
      </button>

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
          'relative flex items-center justify-center shrink-0 text-t-secondary bg-fill-2',
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
        {hasUnread ? (
          <span
            className='absolute top-0 right-0 size-8px rd-full bg-danger border-2 border-solid border-[var(--color-bg-1)]'
            aria-hidden
          />
        ) : null}
      </span>
      {!collapsed && (
        <span className='min-w-0 h-31px flex-1 flex flex-col justify-center gap-1px' data-sider-account-copy>
          <span className='block h-16px truncate text-12px font-500 leading-16px text-t-primary'>{displayName}</span>
          <span
            className={classNames(
              'block h-14px truncate text-11px leading-14px text-t-tertiary',
              !planText && 'invisible'
            )}
            aria-hidden={!planText}
            data-sider-plan-slot
          >
            {planText || '\u00a0'}
          </span>
        </span>
      )}
      {!collapsed && authenticated && (
        <span
          data-sider-credits
          className='shrink-0 pr-4px text-11px leading-14px text-t-tertiary tabular-nums'
        >
          {creditsText}
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
