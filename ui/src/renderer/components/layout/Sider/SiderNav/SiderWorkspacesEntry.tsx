

import { FlowyIcon } from '@renderer/components/base/FlowyIcon';
import React, { useCallback } from 'react';
import { Tooltip } from '@arco-design/web-react';
import classNames from 'classnames';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';

interface SiderWorkspacesEntryProps {
  isMobile: boolean;
  isActive: boolean;
  collapsed: boolean;
  siderTooltipProps: SiderTooltipProps;
  onClick: () => void;
}

/**
 * Workspaces entry — the primary navigation to project/workpath list.
 * Mirrors a "New Agent" style action button in collapsed mode and a
 * label+icon row when expanded.
 */
const SiderWorkspacesEntry: React.FC<SiderWorkspacesEntryProps> = ({
  isMobile,
  isActive,
  collapsed,
  siderTooltipProps,
  onClick,
}) => {
  const label = '工作空间';

  if (collapsed) {
    return (
      <Tooltip {...siderTooltipProps} content={label} position='right'>
        <button
          type='button'
          className={classNames(
            'w-full h-34px flex items-center justify-center cursor-pointer transition-colors rd-8px text-t-primary b-none bg-transparent p-0',
            isActive ? '!bg-primary-1 !text-primary-6' : 'hover:bg-fill-2 active:bg-fill-3'
          )}
          onClick={onClick}
          aria-current={isActive ? 'page' : undefined}
          aria-label={label}
          data-testid='sider-workspaces-entry'
        >
          <FlowyIcon name='bookOpen' size={20} className='block leading-none shrink-0' />
        </button>
      </Tooltip>
    );
  }

  return (
    <Tooltip {...siderTooltipProps} content={label} position='right'>
      <button
        type='button'
        className={classNames(
          'box-border group h-34px w-full flex items-center justify-start gap-8px pl-10px pr-8px rd-0.5rem cursor-pointer shrink-0 transition-colors duration-fast ease-enter motion-reduce:transition-none text-t-primary b-none bg-transparent',
          isMobile && 'sider-action-btn-mobile',
          isActive ? '!bg-primary-1 !text-primary-6' : 'hover:bg-fill-2 active:bg-fill-3'
        )}
        onClick={onClick}
        aria-current={isActive ? 'page' : undefined}
        data-testid='sider-workspaces-entry'
      >
        <span className='size-22px flex items-center justify-center shrink-0'>
          <FlowyIcon name='bookOpen' size={16} className='block leading-none' />
        </span>
        <span className='collapsed-hidden text-14px font-[500] leading-24px'>{label}</span>
      </button>
    </Tooltip>
  );
};

export default SiderWorkspacesEntry;
