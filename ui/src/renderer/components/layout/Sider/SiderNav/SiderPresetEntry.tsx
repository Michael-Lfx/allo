

import React from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@arco-design/web-react';
import { GraduationCap } from 'lucide-react';
import classNames from 'classnames';
import { navRowProps } from './navRowProps';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';

interface SiderPresetEntryProps {
  isMobile: boolean;
  isActive: boolean;
  collapsed: boolean;
  siderTooltipProps: SiderTooltipProps;
  onClick: () => void;
}

/** Presets — reusable launch configurations for Agents, companions and execution steps. */
const SiderPresetEntry: React.FC<SiderPresetEntryProps> = ({
  isMobile,
  isActive,
  collapsed,
  siderTooltipProps,
  onClick,
}) => {
  const { t } = useTranslation();
  const label = t('settings.presetsHub.railTitle', { defaultValue: 'Experts' });

  if (collapsed) {
    return (
      <Tooltip {...siderTooltipProps} content={label} position='right'>
        <div
          className={classNames(
            'w-full h-34px flex items-center justify-center cursor-pointer transition-colors rd-8px text-t-primary',
            isActive ? '!bg-primary-1 !text-primary-6' : 'hover:bg-fill-2 active:bg-fill-3'
          )}
          {...navRowProps(onClick, label)}
          onClick={onClick}
        >
          <GraduationCap size={20} strokeWidth={2} className='block shrink-0' />
        </div>
      </Tooltip>
    );
  }

  return (
    <Tooltip {...siderTooltipProps} content={label} position='right'>
      <div
        className={classNames(
          'box-border group h-34px w-full flex items-center justify-start gap-8px pl-10px pr-8px rd-0.5rem cursor-pointer shrink-0 transition-colors duration-fast ease-enter motion-reduce:transition-none text-t-primary',
          isMobile && 'sider-action-btn-mobile',
          isActive ? '!bg-primary-1 !text-primary-6' : 'hover:bg-fill-2 active:bg-fill-3'
        )}
        {...navRowProps(onClick, label)}
          onClick={onClick}
      >
        <span className='size-22px flex items-center justify-center shrink-0'>
          <GraduationCap size={16} strokeWidth={2} className='block' />
        </span>
        <span className='collapsed-hidden text-14px font-[500] leading-24px'>{label}</span>
      </div>
    </Tooltip>
  );
};

export default SiderPresetEntry;
