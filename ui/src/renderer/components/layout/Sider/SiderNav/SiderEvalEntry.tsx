import React from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@arco-design/web-react';
import { Experiment } from '@icon-park/react';
import classNames from 'classnames';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';

interface SiderEvalEntryProps {
  isMobile: boolean;
  isActive: boolean;
  collapsed: boolean;
  dock?: boolean;
  siderTooltipProps: SiderTooltipProps;
  onClick: () => void;
}

const SiderEvalEntry: React.FC<SiderEvalEntryProps> = ({
  isMobile,
  isActive,
  collapsed,
  dock = false,
  siderTooltipProps,
  onClick,
}) => {
  const { t } = useTranslation();
  const label = t('eval.title');
  const devLabel = t('eval.dev.tag');
  const tooltipContent = t('eval.dev.navTooltip');
  const icon = (
    <Experiment
      theme='outline'
      size={collapsed ? '20' : '16'}
      fill='currentColor'
      className='block leading-none'
      style={{ lineHeight: 0 }}
    />
  );

  if (dock) {
    return (
      <Tooltip {...siderTooltipProps} content={tooltipContent} position='bottom'>
        <div
          className={classNames(
            'size-26px flex items-center justify-center cursor-pointer transition-colors rd-6px text-t-secondary hover:text-t-primary',
            isActive ? '!bg-primary-1 !text-primary-6' : 'hover:bg-fill-2 active:bg-fill-3'
          )}
          onClick={onClick}
          aria-current={isActive ? 'page' : undefined}
          data-sider-nav-entry
          data-active={isActive ? 'true' : 'false'}
          data-sider-selection-static='true'
        >
          <Experiment
            theme='outline'
            size='15'
            fill='currentColor'
            className='block leading-none'
            style={{ lineHeight: 0 }}
          />
        </div>
      </Tooltip>
    );
  }

  return (
    <Tooltip {...siderTooltipProps} content={tooltipContent} position='right'>
      <div
        className={classNames(
          'box-border group h-34px w-full flex items-center cursor-pointer transition-colors rd-8px text-t-primary',
          collapsed
            ? 'justify-center'
            : 'justify-start gap-8px pl-10px pr-8px rd-0.5rem shrink-0',
          isMobile && 'sider-action-btn-mobile',
          isActive ? '!bg-primary-1 !text-primary-6' : 'hover:bg-fill-2 active:bg-fill-3'
        )}
        onClick={onClick}
        aria-current={isActive ? 'page' : undefined}
        data-sider-nav-entry
        data-active={isActive ? 'true' : 'false'}
      >
        {collapsed ? icon : (
          <>
            <span className='size-22px flex items-center justify-center shrink-0'>{icon}</span>
            <span className='collapsed-hidden text-14px font-[500] leading-24px'>{label}</span>
            <span
              className='collapsed-hidden ml-auto shrink-0 text-9px font-600 leading-none tracking-wide uppercase px-4px py-2px rd-4px bg-[rgba(var(--primary-6),0.12)] text-[rgb(var(--primary-6))]'
              aria-hidden='true'
            >
              {devLabel}
            </span>
          </>
        )}
      </div>
    </Tooltip>
  );
};

export default SiderEvalEntry;
