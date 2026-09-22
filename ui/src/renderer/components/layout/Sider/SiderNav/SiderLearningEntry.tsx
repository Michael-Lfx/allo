import React from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@arco-design/web-react';
import { BookOpen } from '@icon-park/react';
import classNames from 'classnames';
import InstantHoverTooltip from '@renderer/components/base/InstantHoverTooltip';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';
import { prefetchLearningPage } from '@renderer/pages/learning/prefetch';

interface SiderLearningEntryProps {
  isMobile: boolean;
  isActive: boolean;
  collapsed: boolean;
  dock?: boolean;
  siderTooltipProps: SiderTooltipProps;
  onClick: () => void;
}

const SiderLearningEntry: React.FC<SiderLearningEntryProps> = ({
  isMobile,
  isActive,
  collapsed,
  dock = false,
  siderTooltipProps,
  onClick,
}) => {
  const { t } = useTranslation();
  const label = t('learning.title');
  const icon = (
    <BookOpen
      theme='outline'
      size={collapsed ? '20' : '16'}
      fill='currentColor'
      className='block leading-none'
      style={{ lineHeight: 0 }}
    />
  );

  if (dock) {
    return (
      <InstantHoverTooltip content={label} position='bottom' className='flex-1 min-w-0'>
        <div
          role='button'
          tabIndex={0}
          className={classNames(
            'group w-full h-26px flex items-center justify-center cursor-pointer transition-colors rd-6px outline-none focus-visible:ring-1 focus-visible:ring-primary-6',
            isActive
              ? 'bg-fill-3 text-primary-6 shadow-sm'
              : 'bg-transparent text-t-tertiary hover:text-t-primary hover:bg-fill-2'
          )}
          onClick={onClick}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault();
              onClick?.();
            }
          }}
          onPointerEnter={() => prefetchLearningPage()}
          aria-label={label}
          aria-current={isActive ? 'page' : undefined}
        >
          <BookOpen
            theme='outline'
            size='15'
            fill='currentColor'
            className={classNames(
              'block leading-none transition-colors duration-180',
              isActive ? 'text-primary-6' : 'text-t-tertiary group-hover:text-t-primary'
            )}
            style={{ lineHeight: 0 }}
          />
        </div>
      </InstantHoverTooltip>
    );
  }

  return (
    <Tooltip {...siderTooltipProps} content={label} position='right'>
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
        onPointerEnter={() => prefetchLearningPage()}
        aria-current={isActive ? 'page' : undefined}
        data-sider-nav-entry
        data-active={isActive ? 'true' : 'false'}
      >
        {collapsed ? icon : (
          <>
            <span className='size-22px flex items-center justify-center shrink-0'>{icon}</span>
            <span className='collapsed-hidden text-14px font-[500] leading-24px'>{label}</span>
          </>
        )}
      </div>
    </Tooltip>
  );
};

export default SiderLearningEntry;
