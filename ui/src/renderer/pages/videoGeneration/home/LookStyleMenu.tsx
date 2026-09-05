/**
 * Look (画风) picker — grouped catalog, optional selection.
 * Writes the session `style` prompt (same catalog as workspace VisualStyleSelect).
 * Poster thumbnails are omitted until we have real look references.
 */
import React, { useMemo, useState } from 'react';
import { CheckSmall, CloseSmall, Search } from '@icon-park/react';
import { useTranslation } from 'react-i18next';
import {
  VISUAL_STYLE_CATEGORIES,
  VISUAL_STYLE_PRESETS,
  visualStyleSelectValue,
  type VisualStyleCategory,
  type VisualStylePreset,
} from '../visualStylePresets';
import styles from './lookStyleMenu.module.css';

export interface LookStyleMenuProps {
  stylePrompt: string;
  onSelect: (stylePrompt: string) => void;
}

type CategoryFilter = 'all' | VisualStyleCategory;

function presetHaystack(
  preset: VisualStylePreset,
  categoryLabel: string,
  localizedName: string
): string {
  return [preset.key, preset.defaultLabel, localizedName, categoryLabel, preset.prompt]
    .join(' ')
    .toLowerCase();
}

const LookStyleMenu: React.FC<LookStyleMenuProps> = ({ stylePrompt, onSelect }) => {
  const { t } = useTranslation();
  const [query, setQuery] = useState('');
  const [categoryFilter, setCategoryFilter] = useState<CategoryFilter>('all');
  const selectedKey = visualStyleSelectValue(stylePrompt);
  const noneSelected = !selectedKey;

  const grouped = useMemo(() => {
    const q = query.trim().toLowerCase();
    return VISUAL_STYLE_CATEGORIES.map((category) => {
      if (categoryFilter !== 'all' && category.id !== categoryFilter) {
        return { category, categoryLabel: '', presets: [] as VisualStylePreset[] };
      }
      const categoryLabel = t(category.labelKey, { defaultValue: category.defaultLabel });
      const presets = VISUAL_STYLE_PRESETS.filter((preset) => {
        if (preset.category !== category.id) return false;
        if (!q) return true;
        const localizedName = t(preset.labelKey, { defaultValue: preset.defaultLabel });
        return presetHaystack(preset, categoryLabel, localizedName).includes(q);
      });
      return { category, categoryLabel, presets };
    }).filter((group) => group.presets.length > 0);
  }, [categoryFilter, query, t]);

  const selectPreset = (preset: VisualStylePreset) => {
    if (selectedKey === preset.key) {
      onSelect('');
      return;
    }
    onSelect(preset.prompt);
  };

  return (
    <div
      className={styles.shell}
      role='listbox'
      aria-label={t('videoGeneration.looks.menuAria', { defaultValue: '选择画风' })}
    >
      <div className={styles.header}>
        <button
          type='button'
          className={`${styles.noneChip} ${noneSelected ? styles.noneChipActive : ''}`}
          aria-pressed={noneSelected}
          onClick={() => onSelect('')}
        >
          {t('videoGeneration.looks.none', { defaultValue: '不使用画风' })}
        </button>
      </div>
      <p className={styles.hint}>
        {t('videoGeneration.looks.hint', {
          defaultValue: '画风锁定人物、场景与成片的统一媒介，可与 Skill 同时使用。可不选。',
        })}
      </p>
      <label className={styles.search}>
        <span className={styles.searchIcon} aria-hidden='true'>
          <Search theme='outline' size={15} fill='currentColor' />
        </span>
        <input
          autoFocus
          type='search'
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={t('videoGeneration.looks.searchPlaceholder', {
            defaultValue: '搜索画风',
          })}
          aria-label={t('videoGeneration.looks.searchAria', {
            defaultValue: '搜索画风',
          })}
        />
        {query ? (
          <button
            type='button'
            className={styles.searchClear}
            aria-label={t('videoGeneration.looks.clearSearch', { defaultValue: '清除搜索' })}
            onClick={() => setQuery('')}
          >
            <CloseSmall theme='outline' size={14} fill='currentColor' />
          </button>
        ) : null}
      </label>
      <div className={styles.chips} role='tablist' aria-label={t('videoGeneration.looks.categoriesAria', { defaultValue: '画风分类' })}>
        <button
          type='button'
          role='tab'
          aria-selected={categoryFilter === 'all'}
          className={`${styles.chip} ${categoryFilter === 'all' ? styles.chipActive : ''}`}
          onClick={() => setCategoryFilter('all')}
        >
          {t('videoGeneration.looks.allCategories', { defaultValue: '全部' })}
        </button>
        {VISUAL_STYLE_CATEGORIES.map((category) => {
          const active = categoryFilter === category.id;
          return (
            <button
              key={category.id}
              type='button'
              role='tab'
              aria-selected={active}
              className={`${styles.chip} ${active ? styles.chipActive : ''}`}
              onClick={() => setCategoryFilter(category.id)}
            >
              {t(category.labelKey, { defaultValue: category.defaultLabel })}
            </button>
          );
        })}
      </div>
      <div className={styles.listScroll}>
        {grouped.length === 0 ? (
          <div className={styles.empty}>
            {t('videoGeneration.looks.empty', { defaultValue: '没有匹配的画风' })}
          </div>
        ) : (
          grouped.map(({ category, categoryLabel, presets }) => (
            <LookGroup
              key={category.id}
              label={categoryLabel}
              showLabel={categoryFilter === 'all'}
              presets={presets}
              selectedKey={selectedKey}
              onSelect={selectPreset}
              t={t}
            />
          ))
        )}
      </div>
    </div>
  );
};

function LookGroup({
  label,
  showLabel,
  presets,
  selectedKey,
  onSelect,
  t,
}: {
  label: string;
  showLabel: boolean;
  presets: VisualStylePreset[];
  selectedKey: string;
  onSelect: (preset: VisualStylePreset) => void;
  t: (key: string, opts?: Record<string, unknown>) => string;
}) {
  return (
    <div>
      {showLabel ? <div className={styles.groupLabel}>{label}</div> : null}
      {presets.map((preset) => {
        const active = selectedKey === preset.key;
        const name = t(preset.labelKey, { defaultValue: preset.defaultLabel });
        return (
          <button
            key={preset.key}
            type='button'
            role='option'
            aria-selected={active}
            className={`${styles.item} ${active ? styles.itemActive : ''}`}
            onClick={() => onSelect(preset)}
          >
            <span className={styles.checkSlot} aria-hidden='true'>
              {active ? <CheckSmall theme='outline' size={14} fill='currentColor' /> : null}
            </span>
            <span className={styles.itemLabel}>{name}</span>
          </button>
        );
      })}
    </div>
  );
}

export default LookStyleMenu;
