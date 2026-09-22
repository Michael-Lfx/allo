import React from 'react';
import classNames from 'classnames';

export interface SiderEmptyPlaceholderProps {
  icon?: React.ReactNode;
  title: string;
  hint?: string;
  action?: {
    label: string;
    onClick: () => void;
    testId?: string;
    onPointerEnter?: () => void;
  };
  className?: string;
}

/**
 * Lightweight empty placeholder designed specifically for the narrow rail Sider.
 * Standardizes typography, spacing, and CTA button sizes across history tabs.
 */
export const SiderEmptyPlaceholder: React.FC<SiderEmptyPlaceholderProps> = ({
  icon,
  title,
  hint,
  action,
  className,
}) => {
  return (
    <div
      className={classNames(
        'flex flex-col items-center justify-center py-20px px-12px text-center gap-6px min-w-0',
        className
      )}
    >
      {icon && (
        <div className='size-28px flex items-center justify-center text-t-tertiary mb-2px shrink-0 select-none'>
          {icon}
        </div>
      )}
      <span className='text-12px font-[500] text-t-secondary select-none leading-16px truncate max-w-full'>
        {title}
      </span>
      {hint && (
        <span
          className='text-11px text-t-tertiary select-none leading-16px w-full max-w-220px px-4px text-center'
          style={{ textWrap: 'balance' }}
        >
          {hint}
        </span>
      )}
      {action && (
        <button
          type='button'
          data-testid={action.testId}
          onClick={action.onClick}
          onPointerEnter={action.onPointerEnter}
          className='mt-2px h-28px px-12px rd-8px text-11px font-[500] bg-fill-2 hover:bg-fill-3 active:bg-fill-4 text-t-primary border border-solid border-[var(--color-border-2)] cursor-pointer transition-colors select-none flex items-center justify-center gap-4px'
        >
          {action.label}
        </button>
      )}
    </div>
  );
};

export default SiderEmptyPlaceholder;
