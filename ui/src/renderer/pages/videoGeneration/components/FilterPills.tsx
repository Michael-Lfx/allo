import React from 'react';

interface FilterPillsProps<K extends string> {
  items: { key: K; label: string }[];
  active: K;
  onChange: (key: K) => void;
  ariaLabel?: string;
}

export default function FilterPills<K extends string>({
  items,
  active,
  onChange,
  ariaLabel,
}: FilterPillsProps<K>) {
  return (
    <div className='flex flex-wrap gap-6px' role='tablist' aria-label={ariaLabel}>
      {items.map((item) => {
        const on = item.key === active;
        return (
          <button
            key={item.key}
            type='button'
            role='tab'
            aria-selected={on}
            className={[
              'm-0 cursor-pointer border border-solid px-12px py-5px text-12px leading-18px transition-colors rd-999px',
              on
                ? 'border-[rgb(var(--primary-6))] bg-[rgb(var(--primary-1))] text-[rgb(var(--primary-6))] font-600'
                : 'border-[var(--color-border-2)] bg-[var(--color-bg-2)] text-[var(--color-text-2)] hover:border-[rgb(var(--primary-5))]',
            ].join(' ')}
            onClick={() => {
              if (!on) onChange(item.key);
            }}
          >
            {item.label}
          </button>
        );
      })}
    </div>
  );
}
