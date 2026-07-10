/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Popover, Tooltip } from '@arco-design/web-react';
import { Theme } from '@icon-park/react';
import classNames from 'classnames';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';
import SiderThemePanel from './SiderThemePanel';

interface SiderThemeControlProps {
  isMobile: boolean;
  collapsed: boolean;
  siderTooltipProps: SiderTooltipProps;
}

const footerButtonClass = (collapsed: boolean, isMobile: boolean, active: boolean) =>
  classNames(
    'h-34px shrink-0 flex items-center justify-center cursor-pointer rd-0.5rem transition-colors',
    collapsed ? 'w-full' : 'w-36px',
    isMobile && 'sider-footer-btn-mobile',
    active ? '!bg-primary-1 !text-primary-6' : 'text-t-secondary hover:bg-fill-2 hover:text-t-primary active:bg-fill-3'
  );

const SiderThemeControl: React.FC<SiderThemeControlProps> = ({ isMobile, collapsed, siderTooltipProps }) => {
  const { t } = useTranslation();
  const [popupVisible, setPopupVisible] = useState(false);

  return (
    <Popover
      className='sider-soft-popover sider-theme-popover'
      trigger='click'
      position={collapsed ? 'rt' : 'top'}
      popupVisible={popupVisible}
      onVisibleChange={setPopupVisible}
      getPopupContainer={() => document.body}
      content={<SiderThemePanel className='w-280px' onBeforeOpenModal={() => setPopupVisible(false)} />}
      unmountOnExit
    >
      <Tooltip {...siderTooltipProps} content={t('settings.theme')} position='right'>
        <div className={footerButtonClass(collapsed, isMobile, popupVisible)} aria-label={t('settings.theme')}>
          <Theme theme='outline' size='18' fill='currentColor' className='block leading-none' style={{ lineHeight: 0 }} />
        </div>
      </Tooltip>
    </Popover>
  );
};

export default SiderThemeControl;
