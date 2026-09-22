

import React from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@arco-design/web-react';
import { BookOne } from '@icon-park/react';
import classNames from 'classnames';
import InstantHoverTooltip from '@renderer/components/base/InstantHoverTooltip';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';

interface SiderKnowledgeEntryProps {
  isMobile: boolean;
  isActive: boolean;
  collapsed: boolean;
  siderTooltipProps: SiderTooltipProps;
  onClick: () => void;
  /** Red dot — set when there are unreviewed knowledge write-back proposals. */
  dot?: boolean;
  dock?: boolean;
}

/** Small red badge dot for the "unreviewed proposals" signal. */
const RedDot: React.FC = () => (
  <span
    className='absolute rounded-full bg-red-500'
    style={{ width: 6, height: 6, top: -1, right: -1 }}
  />
);

const SiderKnowledgeEntry: React.FC<SiderKnowledgeEntryProps> = ({
  isMobile,
  isActive,
  collapsed,
  dock = false,
  siderTooltipProps,
  onClick,
  dot = false,
}) => {
  const { t } = useTranslation();
  const label = t('knowledge.title');

  if (dock) {
    return (
      <InstantHoverTooltip content={label} position='bottom' className='flex-1 min-w-0'>
        <div
          className={classNames(
            'group w-full h-26px flex items-center justify-center cursor-pointer transition-colors rd-6px',
            isActive
              ? 'bg-fill-3 text-primary-6 shadow-sm'
              : 'bg-transparent text-t-tertiary hover:text-t-primary hover:bg-fill-2'
          )}
          onClick={onClick}
          aria-label={label}
          aria-current={isActive ? 'page' : undefined}
        >
          <span className='relative block leading-none shrink-0' style={{ lineHeight: 0 }}>
            <BookOne
              theme='outline'
              size='15'
              fill='currentColor'
              className={classNames(
                'block leading-none transition-colors duration-180',
                isActive ? 'text-primary-6' : 'text-t-tertiary group-hover:text-t-primary'
              )}
            />
            {dot && <RedDot />}
          </span>
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
          aria-current={isActive ? 'page' : undefined}
          data-sider-nav-entry
          data-active={isActive ? 'true' : 'false'}
        >
          <span className='relative block leading-none shrink-0' style={{ lineHeight: 0 }}>
            <BookOne theme='outline' size='20' fill='currentColor' className='block leading-none' />
            {dot && <RedDot />}
          </span>
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
        aria-current={isActive ? 'page' : undefined}
        data-sider-nav-entry
        data-active={isActive ? 'true' : 'false'}
      >
        <span className='relative size-22px flex items-center justify-center shrink-0'>
          <BookOne
            theme='outline'
            size='16'
            fill='currentColor'
            className='block leading-none'
            style={{ lineHeight: 0 }}
          />
          {dot && <RedDot />}
        </span>
        <span className='collapsed-hidden text-14px font-[500] leading-24px'>{label}</span>
      </div>
    </Tooltip>
  );
};

export default SiderKnowledgeEntry;
