import React from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@arco-design/web-react';
import { BookOpen } from '@icon-park/react';
import classNames from 'classnames';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';

interface SiderLearningEntryProps {
  isMobile: boolean;
  isActive: boolean;
  collapsed: boolean;
  siderTooltipProps: SiderTooltipProps;
  onClick: () => void;
}

const SiderLearningEntry: React.FC<SiderLearningEntryProps> = ({
  isMobile,
  isActive,
  collapsed,
  siderTooltipProps,
  onClick,
}) => {
  const { t } = useTranslation();
  const label = t('learning.title');
  const devLabel = t('learning.dev.tag');
  const tooltipContent = t('learning.dev.navTooltip');
  const icon = (
    <BookOpen
      theme='outline'
      size={collapsed ? '20' : '16'}
      fill='currentColor'
      className='block leading-none'
      style={{ lineHeight: 0 }}
    />
  );

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

export default SiderLearningEntry;
