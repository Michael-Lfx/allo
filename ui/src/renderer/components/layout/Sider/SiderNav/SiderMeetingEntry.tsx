import React, { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@arco-design/web-react';
import { Voice } from '@icon-park/react';
import classNames from 'classnames';
import InstantHoverTooltip from '@renderer/components/base/InstantHoverTooltip';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';
import { prefetchMeetingPage } from '@renderer/pages/meeting/prefetch';

interface SiderMeetingEntryProps {
  isMobile: boolean;
  isActive: boolean;
  collapsed: boolean;
  dock?: boolean;
  siderTooltipProps: SiderTooltipProps;
  onClick: () => void;
}

const SiderMeetingEntry: React.FC<SiderMeetingEntryProps> = ({
  isMobile,
  isActive,
  collapsed,
  dock = false,
  siderTooltipProps,
  onClick,
}) => {
  const { t } = useTranslation();
  const label = t('meeting.title');

  useEffect(() => {
    const idleWindow = window as Window & {
      requestIdleCallback?: (callback: () => void, options?: { timeout?: number }) => number;
      cancelIdleCallback?: (handle: number) => void;
    };
    if (typeof idleWindow.requestIdleCallback === 'function') {
      const idleId = idleWindow.requestIdleCallback(() => prefetchMeetingPage(), {
        timeout: 1800,
      });
      return () => idleWindow.cancelIdleCallback?.(idleId);
    }
    const timer = window.setTimeout(() => prefetchMeetingPage(), 250);
    return () => window.clearTimeout(timer);
  }, []);

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
          onPointerEnter={() => prefetchMeetingPage()}
          aria-label={label}
          aria-current={isActive ? 'page' : undefined}
        >
          <Voice
            theme='outline'
            size='15'
            fill='currentColor'
            className={classNames(
              'block leading-none shrink-0 transition-colors duration-180',
              isActive ? 'text-primary-6' : 'text-t-tertiary group-hover:text-t-primary'
            )}
            style={{ lineHeight: 0 }}
          />
        </div>
      </InstantHoverTooltip>
    );
  }

  if (collapsed) {
    return (
      <Tooltip {...siderTooltipProps} content={label} position='right'>
        <div
          className={classNames(
            'w-full h-34px flex items-center justify-center cursor-pointer transition-colors rd-8px text-t-primary',
            isActive ? '!bg-primary-1 !text-primary-6' : 'hover:bg-fill-2 active:bg-fill-3'
          )}
          onClick={onClick}
          onPointerEnter={() => prefetchMeetingPage()}
          aria-current={isActive ? 'page' : undefined}
          data-sider-nav-entry
          data-active={isActive ? 'true' : 'false'}
        >
          <Voice
            theme='outline'
            size='20'
            fill='currentColor'
            className='block leading-none shrink-0'
            style={{ lineHeight: 0 }}
          />
        </div>
      </Tooltip>
    );
  }

  return (
    <Tooltip {...siderTooltipProps} content={label} position='right'>
      <div
        className={classNames(
          'box-border group h-34px w-full flex items-center justify-start gap-8px pl-10px pr-8px rd-0.5rem cursor-pointer shrink-0 transition-all text-t-primary',
          isMobile && 'sider-action-btn-mobile',
          isActive ? '!bg-primary-1 !text-primary-6' : 'hover:bg-fill-2 active:bg-fill-3'
        )}
        onClick={onClick}
        onPointerEnter={() => prefetchMeetingPage()}
        aria-current={isActive ? 'page' : undefined}
        data-sider-nav-entry
        data-active={isActive ? 'true' : 'false'}
      >
        <span className='size-22px flex items-center justify-center shrink-0'>
          <Voice
            theme='outline'
            size='16'
            fill='currentColor'
            className='block leading-none'
            style={{ lineHeight: 0 }}
          />
        </span>
        <span className='collapsed-hidden text-14px font-[500] leading-24px'>{label}</span>
      </div>
    </Tooltip>
  );
};

export default SiderMeetingEntry;
