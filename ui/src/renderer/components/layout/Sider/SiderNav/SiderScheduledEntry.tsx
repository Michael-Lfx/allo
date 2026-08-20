

import React, { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@arco-design/web-react';
import { AlarmClock } from '@icon-park/react';
import classNames from 'classnames';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';
import { prefetchScheduledTasksPage } from '@renderer/pages/cron/prefetch';

interface SiderScheduledEntryProps {
  isMobile: boolean;
  isActive: boolean;
  collapsed: boolean;
  siderTooltipProps: SiderTooltipProps;
  onClick: () => void;
}

const SiderScheduledEntry: React.FC<SiderScheduledEntryProps> = ({
  isMobile,
  isActive,
  collapsed,
  siderTooltipProps,
  onClick,
}) => {
  const { t } = useTranslation();

  useEffect(() => {
    const idleWindow = window as Window & {
      requestIdleCallback?: (callback: () => void, options?: { timeout?: number }) => number;
      cancelIdleCallback?: (handle: number) => void;
    };
    if (typeof idleWindow.requestIdleCallback === 'function') {
      const idleId = idleWindow.requestIdleCallback(() => prefetchScheduledTasksPage(), {
        timeout: 1800,
      });
      return () => idleWindow.cancelIdleCallback?.(idleId);
    }
    const timer = window.setTimeout(() => prefetchScheduledTasksPage(), 250);
    return () => window.clearTimeout(timer);
  }, []);

  if (collapsed) {
    return (
      <Tooltip {...siderTooltipProps} content={t('cron.scheduledTasks')} position='right'>
        <div
          className={classNames(
            'w-full h-34px flex items-center justify-center cursor-pointer transition-colors rd-8px text-t-primary',
            isActive ? '!bg-primary-1 !text-primary-6' : 'hover:bg-fill-2 active:bg-fill-3'
          )}
          onClick={onClick}
          onPointerEnter={() => prefetchScheduledTasksPage()}
          aria-current={isActive ? 'page' : undefined}
          data-sider-nav-entry
          data-active={isActive ? 'true' : 'false'}
        >
          <AlarmClock
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
    <Tooltip {...siderTooltipProps} content={t('cron.scheduledTasks')} position='right'>
      <div
        className={classNames(
          'box-border group h-34px w-full flex items-center justify-start gap-8px pl-10px pr-8px rd-0.5rem cursor-pointer shrink-0 transition-all text-t-primary',
          isMobile && 'sider-action-btn-mobile',
          isActive ? '!bg-primary-1 !text-primary-6' : 'hover:bg-fill-2 active:bg-fill-3'
        )}
        onClick={onClick}
        onPointerEnter={() => prefetchScheduledTasksPage()}
        aria-current={isActive ? 'page' : undefined}
        data-sider-nav-entry
        data-active={isActive ? 'true' : 'false'}
      >
        <span className='size-22px flex items-center justify-center shrink-0'>
          <AlarmClock
            theme='outline'
            size='16'
            fill='currentColor'
            className='block leading-none'
            style={{ lineHeight: 0 }}
          />
        </span>
        <span className='collapsed-hidden text-14px font-[500] leading-24px'>
          {t('cron.scheduledTasks')}
        </span>
      </div>
    </Tooltip>
  );
};

export default SiderScheduledEntry;
