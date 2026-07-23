

import React from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@arco-design/web-react';
import { ArrowCircleLeft, SettingTwo } from '@icon-park/react';
import classNames from 'classnames';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';
import SiderUserMenu from './SiderUserMenu';

interface SiderFooterProps {
  isMobile: boolean;
  isSettings: boolean;
  collapsed?: boolean;
  siderTooltipProps: SiderTooltipProps;
  userLabel?: string;
  planLabel?: string;
  showLogout?: boolean;
  onLogout?: () => void;
  onSettingsClick: () => void;
}

const iconButtonClass = (collapsed: boolean, isMobile: boolean, active: boolean) =>
  classNames(
    'h-34px shrink-0 flex items-center justify-center cursor-pointer rd-0.5rem transition-colors',
    collapsed ? 'w-full' : 'w-32px',
    isMobile && 'sider-footer-btn-mobile',
    active ? '!bg-primary-1 !text-primary-6' : 'text-t-secondary hover:bg-fill-2 hover:text-t-primary active:bg-fill-3'
  );

const SiderFooter: React.FC<SiderFooterProps> = ({
  isMobile,
  isSettings,
  collapsed = false,
  siderTooltipProps,
  userLabel,
  planLabel,
  showLogout = false,
  onLogout,
  onSettingsClick,
}) => {
  const { t } = useTranslation();
  const settingsTooltip = isSettings ? t('common.back') : t('common.settings');

  const settingsIcon = isSettings ? (
    <ArrowCircleLeft
      theme='outline'
      size='16'
      fill='currentColor'
      className='block leading-none'
      style={{ lineHeight: 0 }}
    />
  ) : (
    <SettingTwo
      theme='outline'
      size='16'
      fill='currentColor'
      className='block leading-none'
      style={{ lineHeight: 0 }}
    />
  );

  const settingsControl = (
    <Tooltip {...siderTooltipProps} content={settingsTooltip} position='right'>
      <div onClick={onSettingsClick} className={iconButtonClass(collapsed, isMobile, isSettings)}>
        {settingsIcon}
      </div>
    </Tooltip>
  );

  return (
    <div className='shrink-0 sider-footer pb-8px'>
      {collapsed ? (
        <div className='flex flex-col gap-2px'>
          <SiderUserMenu
            isMobile={isMobile}
            collapsed={collapsed}
            siderTooltipProps={siderTooltipProps}
            userLabel={userLabel}
            planLabel={planLabel}
            showLogout={showLogout}
            onLogout={onLogout}
          />
          {settingsControl}
        </div>
      ) : (
        <div className='flex items-center gap-2px min-w-0'>
          <SiderUserMenu
            isMobile={isMobile}
            collapsed={collapsed}
            siderTooltipProps={siderTooltipProps}
            userLabel={userLabel}
            planLabel={planLabel}
            showLogout={showLogout}
            onLogout={onLogout}
          />
          <div className='shrink-0 flex items-center'>{settingsControl}</div>
        </div>
      )}
    </div>
  );
};

export default SiderFooter;
