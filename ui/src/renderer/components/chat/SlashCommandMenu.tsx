

import classNames from 'classnames';
import React, { useLayoutEffect, useRef } from 'react';

export interface SlashCommandMenuItem {
  key: string;
  label: string;
  description?: string;
  badge?: string;
  section?: string;
  icon?: React.ReactNode;
}

interface SlashCommandMenuProps {
  title: string;
  hint?: string;
  /** Launcher menus omit the redundant header and use denser rows. */
  compact?: boolean;
  items: SlashCommandMenuItem[];
  activeIndex: number;
  loading?: boolean;
  loadingText?: string;
  onHoverItem: (index: number) => void;
  onSelectItem: (item: SlashCommandMenuItem) => void;
  emptyText: string;
}

const SlashCommandMenu: React.FC<SlashCommandMenuProps> = ({
  title,
  hint,
  compact = false,
  items,
  activeIndex,
  loading = false,
  loadingText = 'Loading...',
  onHoverItem,
  onSelectItem,
  emptyText,
}) => {
  const itemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const listRef = useRef<HTMLDivElement | null>(null);
  // Scrolling the list can cause Chromium to re-hit-test a stationary pointer
  // against a different row. Only a real pointer movement should change the
  // hover selection, otherwise keyboard navigation can oscillate.
  const lastPointerPositionRef = useRef<{ x: number; y: number } | null>(null);

  const handlePointerMove = (event: React.PointerEvent<HTMLButtonElement>, index: number) => {
    const nextPosition = { x: event.clientX, y: event.clientY };
    const previousPosition = lastPointerPositionRef.current;
    lastPointerPositionRef.current = nextPosition;
    if (previousPosition && previousPosition.x === nextPosition.x && previousPosition.y === nextPosition.y) {
      return;
    }
    onHoverItem(index);
  };

  useLayoutEffect(() => {
    const list = listRef.current;
    const current = itemRefs.current[activeIndex];
    if (!list || !current) {
      return;
    }

    const listBounds = list.getBoundingClientRect();
    const itemBounds = current.getBoundingClientRect();
    if (itemBounds.top < listBounds.top) {
      list.scrollTop += itemBounds.top - listBounds.top;
    } else if (itemBounds.bottom > listBounds.bottom) {
      list.scrollTop += itemBounds.bottom - listBounds.bottom;
    }
  }, [activeIndex, items.length]);

  return (
    <div
      className={compact ? 'rounded-20px border border-solid overflow-hidden' : 'rounded-14px border border-solid overflow-hidden'}
      style={{
        borderColor: compact
          ? 'color-mix(in srgb, var(--color-border-2) 68%, var(--color-bg-1))'
          : 'var(--color-border-2)',
        background: compact
          ? 'color-mix(in srgb, var(--color-bg-1) 96%, var(--color-fill-1))'
          : 'color-mix(in srgb, var(--color-bg-1) 78%, transparent)',
        boxShadow: compact
          ? '0 8px 24px color-mix(in srgb, var(--color-text-1) 6%, transparent)'
          : '0 8px 24px rgba(0,0,0,0.12)',
        backdropFilter: compact ? undefined : 'blur(14px) saturate(1.1)',
        WebkitBackdropFilter: compact ? undefined : 'blur(14px) saturate(1.1)',
      }}
    >
      {!compact && (
        <div
          className='px-12px py-8px border-b border-b-solid flex items-center justify-between gap-8px'
          style={{
            borderColor: 'color-mix(in srgb, var(--color-border-2) 56%, transparent)',
            background: 'color-mix(in srgb, var(--color-bg-1) 84%, transparent)',
          }}
        >
          <div className='text-13px font-semibold text-t-primary'>{title}</div>
          {hint && <div className='text-13px text-t-secondary truncate'>{hint}</div>}
        </div>
      )}
      <div
        ref={listRef}
        role='listbox'
        aria-busy={loading}
        className={compact ? 'overflow-y-auto px-8px py-4px' : 'overflow-y-auto p-6px'}
        style={{ maxHeight: compact ? 'min(32vh, 232px)' : 'min(34vh, 260px)' }}
        onPointerLeave={() => {
          lastPointerPositionRef.current = null;
        }}
      >
        {loading && (
          <div className={compact ? 'px-8px py-8px text-12px text-t-tertiary' : 'px-10px py-12px text-13px text-t-secondary'}>
            {loadingText}
          </div>
        )}
        {!loading && items.length === 0 && (
          <div className={compact ? 'px-8px py-8px text-12px text-t-tertiary' : 'px-10px py-12px text-13px text-t-secondary'}>
            {emptyText}
          </div>
        )}
        {!loading &&
          items.map((item, index) => (
            <React.Fragment key={item.key}>
              {item.section && (index === 0 || items[index - 1]?.section !== item.section) && (
                <div className={compact ? 'px-8px pt-4px pb-2px text-10px text-t-tertiary' : 'px-10px pt-6px pb-4px text-11px text-t-tertiary'}>
                  {item.section}
                </div>
              )}
              <button
                type='button'
                role='option'
                aria-selected={index === activeIndex}
                ref={(node) => {
                  itemRefs.current[index] = node;
                }}
                className={classNames(
                  compact
                    ? 'w-full text-left px-10px py-2px rounded-10px transition-none border border-solid outline-none cursor-pointer mb-1px last:mb-0'
                    : 'w-full text-left px-10px py-6px rounded-8px transition-none border border-solid outline-none cursor-pointer mb-2px last:mb-0',
                  {
                    'border-transparent': true,
                    'hover:bg-[var(--color-fill-1)]': index !== activeIndex,
                  }
                )}
                style={{
                  minHeight: compact ? '28px' : '38px',
                  background:
                    index === activeIndex
                      ? 'color-mix(in srgb, var(--color-fill-2) 76%, var(--color-bg-1))'
                      : 'transparent',
                  boxShadow: undefined,
                }}
                onPointerMove={(event) => handlePointerMove(event, index)}
                onClick={() => onSelectItem(item)}
              >
                <div className={compact ? 'flex items-center justify-between gap-6px' : 'flex items-center justify-between gap-8px'}>
                  <div
                    className={
                      item.icon
                        ? 'min-w-0 flex flex-1 items-center gap-8px'
                        : compact
                          ? 'min-w-0 flex items-baseline gap-8px'
                          : 'min-w-0 flex items-baseline gap-10px'
                    }
                  >
                    {item.icon && <span className='flex h-18px w-18px flex-shrink-0 items-center justify-center'>{item.icon}</span>}
                    <div
                      style={item.icon ? undefined : { display: 'contents' }}
                      className={compact ? 'min-w-0 flex items-baseline gap-8px' : 'min-w-0 flex items-baseline gap-10px'}
                    >
                    <div
                      className={classNames(
                        compact ? 'text-13px whitespace-nowrap' : 'text-14px whitespace-nowrap',
                        index === activeIndex ? 'text-t-primary font-semibold' : compact ? 'text-t-secondary font-medium' : 'text-t-primary font-medium'
                      )}
                    >
                      {item.label}
                    </div>
                    {item.description && (
                      <div className={compact ? 'text-11px text-t-tertiary truncate' : 'text-12px text-t-secondary truncate'}>
                        {item.description}
                      </div>
                    )}
                    </div>
                  </div>
                  {item.badge && (
                    <span
                      className={classNames(
                        compact
                          ? 'text-12px leading-20px shrink-0'
                          : 'text-10px rounded-999px px-6px py-1px shrink-0',
                        compact
                          ? 'text-t-tertiary'
                          : index === activeIndex
                            ? 'text-t-secondary bg-[var(--color-bg-1)]'
                            : 'text-t-tertiary bg-[var(--color-bg-1)]'
                      )}
                    >
                      {item.badge}
                    </span>
                  )}
                </div>
              </button>
            </React.Fragment>
          ))}
      </div>
    </div>
  );
};

export default SlashCommandMenu;
