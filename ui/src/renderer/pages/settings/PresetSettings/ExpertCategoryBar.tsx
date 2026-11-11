import { Left, Right } from '@icon-park/react';
import React, { useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  EXPERT_CATEGORIES,
  type ExpertCategoryFilter,
} from './expertCatalogModel';

type ExpertCategoryBarProps = {
  active: ExpertCategoryFilter;
  onChange: (next: ExpertCategoryFilter) => void;
};

const ExpertCategoryBar: React.FC<ExpertCategoryBarProps> = ({ active, onChange }) => {
  const { t } = useTranslation();
  const scrollerRef = useRef<HTMLDivElement>(null);
  const [canScrollLeft, setCanScrollLeft] = useState(false);
  const [canScrollRight, setCanScrollRight] = useState(false);

  useLayoutEffect(() => {
    const el = scrollerRef.current;
    if (!el) return;
    const update = () => {
      setCanScrollLeft(el.scrollLeft > 4);
      setCanScrollRight(el.scrollWidth - el.scrollLeft - el.clientWidth > 4);
    };
    update();
    el.addEventListener('scroll', update, { passive: true });
    const observer = new ResizeObserver(update);
    observer.observe(el);
    return () => {
      el.removeEventListener('scroll', update);
      observer.disconnect();
    };
  }, []);

  const items: { key: ExpertCategoryFilter; label: string }[] = [
    { key: 'all', label: t('settings.experts.all', { defaultValue: 'All' }) },
    ...EXPERT_CATEGORIES.map((category) => ({
      key: category.id,
      label: t(category.labelKey),
    })),
  ];

  return (
    <div className='relative min-w-0 max-w-full' data-testid='expert-category-bar'>
      <div
        ref={scrollerRef}
        role='tablist'
        aria-label={t('settings.experts.filterLabel', { defaultValue: 'Expert category' })}
        className='flex max-w-full items-center gap-4px overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden'
      >
        {items.map((item) => {
          const selected = item.key === active;
          return (
            <button
              key={item.key}
              type='button'
              role='tab'
              aria-selected={selected}
              data-testid={`expert-category-${item.key}`}
              onClick={() => {
                if (item.key !== active) onChange(item.key);
              }}
              className={[
                'shrink-0 cursor-pointer rounded-8px border-none px-12px py-6px text-13px leading-20px whitespace-nowrap',
                'transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgba(var(--primary-6),0.28)]',
                selected
                  ? 'bg-fill-2 font-medium text-t-primary'
                  : 'bg-transparent text-t-secondary hover:text-t-primary',
              ].join(' ')}
            >
              {item.label}
            </button>
          );
        })}
      </div>
      {canScrollLeft ? (
        <button
          type='button'
          aria-label={t('settings.experts.scrollCategoriesPrev', { defaultValue: 'Previous categories' })}
          className='absolute left-0 top-1/2 z-10 inline-flex size-24px -translate-y-1/2 cursor-pointer items-center justify-center rounded-full border-none bg-[var(--color-bg-1)] text-t-secondary'
          onClick={() => scrollerRef.current?.scrollBy({ left: -220, behavior: 'smooth' })}
        >
          <Left theme='outline' size={12} />
        </button>
      ) : null}
      {canScrollRight ? (
        <button
          type='button'
          aria-label={t('settings.experts.scrollCategories', { defaultValue: 'More categories' })}
          className='absolute right-0 top-1/2 z-10 inline-flex size-24px -translate-y-1/2 cursor-pointer items-center justify-center rounded-full border-none bg-[var(--color-bg-1)] text-t-secondary'
          onClick={() => scrollerRef.current?.scrollBy({ left: 220, behavior: 'smooth' })}
        >
          <Right theme='outline' size={12} />
        </button>
      ) : null}
    </div>
  );
};

export default ExpertCategoryBar;
