

import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@arco-design/web-react';
import { Config, Down, Right, Robot, Puzzle, Tool } from '@icon-park/react';
import classNames from 'classnames';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';

type ConfigChild = {
  id: 'presets' | 'skills' | 'mcp';
  label: string;
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
};

interface SiderConfigGroupProps {
  isMobile: boolean;
  collapsed: boolean;
  siderTooltipProps: SiderTooltipProps;
  presetsActive: boolean;
  skillsActive: boolean;
  mcpActive: boolean;
  onPresets: () => void;
  onSkills: () => void;
  onMcp: () => void;
}

/** Config One secondary nav: Presets / Skills / MCP under one rail entry. */
const SiderConfigGroup: React.FC<SiderConfigGroupProps> = ({
  isMobile,
  collapsed,
  siderTooltipProps,
  presetsActive,
  skillsActive,
  mcpActive,
  onPresets,
  onSkills,
  onMcp,
}) => {
  const { t } = useTranslation();
  const anyActive = presetsActive || skillsActive || mcpActive;
  const [expanded, setExpanded] = useState(anyActive);

  useEffect(() => {
    if (anyActive) setExpanded(true);
  }, [anyActive]);

  const groupLabel = t('common.siderSection.config', { defaultValue: 'Config' });
  const children: ConfigChild[] = [
    {
      id: 'presets',
      label: t('settings.presetsHub.railTitle', { defaultValue: 'Presets' }),
      active: presetsActive,
      onClick: onPresets,
      icon: <Robot theme='outline' size='14' fill='currentColor' className='block leading-none' style={{ lineHeight: 0 }} />,
    },
    {
      id: 'skills',
      label: t('settings.skillsHub.railTitle', { defaultValue: 'Skills' }),
      active: skillsActive,
      onClick: onSkills,
      icon: <Puzzle theme='outline' size='14' fill='currentColor' className='block leading-none' style={{ lineHeight: 0 }} />,
    },
    {
      id: 'mcp',
      label: t('settings.mcpHub.railTitle', { defaultValue: 'MCP' }),
      active: mcpActive,
      onClick: onMcp,
      icon: <Tool theme='outline' size='14' fill='currentColor' className='block leading-none' style={{ lineHeight: 0 }} />,
    },
  ];

  if (collapsed) {
    return (
      <Tooltip {...siderTooltipProps} content={groupLabel} position='right'>
        <div
          className={classNames(
            'w-full h-34px flex items-center justify-center cursor-pointer transition-colors rd-8px text-t-primary',
            anyActive ? '!bg-primary-1 !text-primary-6' : 'hover:bg-fill-2 active:bg-fill-3'
          )}
          onClick={onPresets}
          data-testid='sider-config-collapsed'
        >
          <Config theme='outline' size='20' fill='currentColor' className='block leading-none shrink-0' style={{ lineHeight: 0 }} />
        </div>
      </Tooltip>
    );
  }

  return (
    <div className='flex flex-col gap-2px' data-testid='sider-config-group'>
      <button
        type='button'
        className={classNames(
          'box-border h-34px w-full flex items-center justify-start gap-8px pl-10px pr-8px rd-0.5rem cursor-pointer shrink-0 transition-all text-t-primary',
          isMobile && 'sider-action-btn-mobile',
          anyActive && !expanded ? '!bg-primary-1 !text-primary-6' : 'hover:bg-fill-2 active:bg-fill-3'
        )}
        onClick={() => setExpanded((v) => !v)}
        aria-expanded={expanded}
      >
        <span className='size-22px flex items-center justify-center shrink-0'>
          <Config theme='outline' size='16' fill='currentColor' className='block leading-none' style={{ lineHeight: 0 }} />
        </span>
        <span className='collapsed-hidden text-14px font-[500] leading-24px flex-1 text-left'>{groupLabel}</span>
        <span className='size-16px flex items-center justify-center text-t-tertiary'>
          {expanded ? (
            <Down theme='outline' size='12' fill='currentColor' />
          ) : (
            <Right theme='outline' size='12' fill='currentColor' />
          )}
        </span>
      </button>
      {expanded &&
        children.map((child) => (
          <button
            key={child.id}
            type='button'
            data-testid={`sider-config-${child.id}`}
            className={classNames(
              'box-border h-30px w-full flex items-center justify-start gap-8px pl-28px pr-8px rd-0.5rem cursor-pointer shrink-0 transition-all text-t-primary',
              isMobile && 'sider-action-btn-mobile',
              child.active ? '!bg-primary-1 !text-primary-6' : 'hover:bg-fill-2 active:bg-fill-3'
            )}
            onClick={child.onClick}
          >
            <span className='size-18px flex items-center justify-center shrink-0'>{child.icon}</span>
            <span className='text-13px font-[500] leading-20px'>{child.label}</span>
          </button>
        ))}
    </div>
  );
};

export default SiderConfigGroup;
