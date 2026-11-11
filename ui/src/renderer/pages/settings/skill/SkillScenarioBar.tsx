import type { PresetTag } from '@/common/types/agent/presetTypes';
import type { PresetTagId } from '@/common/types/ids';
import { Left, Right } from '@icon-park/react';
import React, { useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

type SkillScenarioBarProps = {
  tags: PresetTag[];
  localeKey: string;
  activeId: PresetTagId | null;
  onChange: (id: PresetTagId | null) => void;
};

const labelOf = (tag: PresetTag, localeKey: string): string => tag.label_i18n?.[localeKey] || tag.label;

/** Single-select scenario strip, matching the expert category scroller. */
const SkillScenarioBar: React.FC<SkillScenarioBarProps> = ({ tags, localeKey, activeId, onChange }) => {
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
  }, [tags]);

  return (
    <div className='relative min-w-0 max-w-full' data-testid='skill-scenario-bar'>
      <div
        ref={scrollerRef}
        role='tablist'
        aria-label={t('settings.presetTagScenario', { defaultValue: 'Skill Scenario' })}
        className='flex max-w-full items-center gap-4px overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden'
      >
        <button
          type='button'
          role='tab'
          aria-selected={activeId === null}
          data-testid='skill-scenario-all'
          onClick={() => {
            if (activeId !== null) onChange(null);
          }}
          className={[
            'shrink-0 cursor-pointer rounded-8px border-none px-12px py-6px text-13px leading-20px whitespace-nowrap',
            'transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgba(var(--primary-6),0.28)]',
            activeId === null
              ? 'bg-fill-2 font-medium text-t-primary'
              : 'bg-transparent text-t-secondary hover:text-t-primary',
          ].join(' ')}
        >
          {t('settings.presetTagAll', { defaultValue: 'All' })}
        </button>
        {tags.map((tag) => {
          const selected = tag.preset_tag_id === activeId;
          return (
            <button
              key={tag.preset_tag_id}
              type='button'
              role='tab'
              aria-selected={selected}
              data-testid={`skill-scenario-${tag.key}`}
              onClick={() => {
                if (!selected) onChange(tag.preset_tag_id);
              }}
              className={[
                'shrink-0 cursor-pointer rounded-8px border-none px-12px py-6px text-13px leading-20px whitespace-nowrap',
                'transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgba(var(--primary-6),0.28)]',
                selected
                  ? 'bg-fill-2 font-medium text-t-primary'
                  : 'bg-transparent text-t-secondary hover:text-t-primary',
              ].join(' ')}
            >
              {labelOf(tag, localeKey)}
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

export default SkillScenarioBar;
