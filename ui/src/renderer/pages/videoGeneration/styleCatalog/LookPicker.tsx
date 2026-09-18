import { useMemo, useState } from 'react';
import { CheckSmall, CloseSmall, Search } from '@icon-park/react';
import { useTranslation } from 'react-i18next';
import { CanvasStyleCoverSwatch } from '@oc/components/canvas/canvas-style-cover';
import { ComposerAnchoredOverlay } from '../home/ComposerAnchoredOverlay';
import {
  VISUAL_STYLE_CATEGORIES,
  visualStyleSelectValue,
  type VisualStyleCategory,
} from '../visualStylePresets';
import {
  featuredLooks,
  groupLooks,
  HOME_LOOKS,
  lookByPrompt,
  type UnifiedLook,
} from './looks';
import styles from './lookPicker.module.css';

export type LookPickerProps = {
  stylePrompt: string;
  onSelect: (stylePrompt: string) => void;
  allowEmpty?: boolean;
  disabled?: boolean;
};

type CategoryFilter = 'all' | VisualStyleCategory;

function lookName(look: UnifiedLook, t: (key: string, opts?: Record<string, unknown>) => string) {
  return t(look.labelKey, { defaultValue: look.defaultLabel });
}

export default function LookPicker({
  stylePrompt,
  onSelect,
  allowEmpty = true,
  disabled,
}: LookPickerProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState('');
  const [categoryFilter, setCategoryFilter] = useState<CategoryFilter>('all');
  const selectedKey = visualStyleSelectValue(stylePrompt);
  const selectedLook = lookByPrompt.get(stylePrompt.trim());
  const noneSelected = !selectedLook && selectedKey !== '__custom__';
  const q = query.trim().toLowerCase();

  const visible = useMemo(() => {
    if (!q) return HOME_LOOKS;
    return HOME_LOOKS.filter((look) => {
      const category = VISUAL_STYLE_CATEGORIES.find((item) => item.id === look.category);
      const categoryLabel = category
        ? t(category.labelKey, { defaultValue: category.defaultLabel })
        : '';
      return [look.id, look.defaultLabel, lookName(look, t), categoryLabel, look.modelPrompt]
        .join(' ')
        .toLowerCase()
        .includes(q);
    });
  }, [q, t]);

  const grouped = useMemo(
    () => groupLooks(visible, categoryFilter),
    [categoryFilter, visible],
  );
  const featured = !q && categoryFilter === 'all' ? featuredLooks() : [];

  const selectLook = (look: UnifiedLook) => {
    if (disabled) return;
    if (selectedLook?.id === look.id && allowEmpty) {
      onSelect('');
      return;
    }
    onSelect(look.modelPrompt);
  };

  return (
    <div
      className={styles.shell}
      role='listbox'
      aria-disabled={disabled || undefined}
      aria-label={t('videoGeneration.looks.menuAria', { defaultValue: '选择画风' })}
    >
      {allowEmpty ? (
        <div className={styles.header}>
          <button
            type='button'
            className={`${styles.noneChip} ${noneSelected ? styles.noneChipActive : ''}`}
            aria-pressed={noneSelected}
            disabled={disabled}
            onClick={() => onSelect('')}
          >
            {t('videoGeneration.looks.none', { defaultValue: '不使用画风' })}
          </button>
        </div>
      ) : null}
      <p className={styles.hint}>
        {allowEmpty
          ? t('videoGeneration.looks.hint', {
              defaultValue: '画风锁定人物、场景与成片的统一媒介，可与 Skill 同时使用。可不选。',
            })
          : t('videoGeneration.looks.hintRequired', {
              defaultValue: '画风锁定整片媒介。创作与画布需要选定一项。',
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
          disabled={disabled}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={t('videoGeneration.looks.searchPlaceholder', { defaultValue: '搜索画风' })}
          aria-label={t('videoGeneration.looks.searchAria', { defaultValue: '搜索画风' })}
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
        {featured.length ? (
          <div className={styles.featured} aria-label={t('videoGeneration.looks.featuredAria', { defaultValue: '常用画风' })}>
            <div className={styles.groupLabel}>
              {t('videoGeneration.looks.featured', { defaultValue: '常用' })}
            </div>
            <div className={styles.featuredRow}>
              {featured.map((look) => (
                <button
                  key={look.id}
                  type='button'
                  className={`${styles.featuredChip} ${selectedLook?.id === look.id ? styles.featuredChipActive : ''}`}
                  onClick={() => selectLook(look)}
                >
                  <CanvasStyleCoverSwatch
                    cover={look.cover}
                    className={styles.featuredCover}
                    alt=''
                  />
                  {lookName(look, t)}
                </button>
              ))}
            </div>
          </div>
        ) : null}
        {grouped.length === 0 ? (
          <div className={styles.empty}>
            {t('videoGeneration.looks.empty', { defaultValue: '没有匹配的画风' })}
          </div>
        ) : (
          grouped.map((group) => {
            const meta = VISUAL_STYLE_CATEGORIES.find((item) => item.id === group.id);
            return (
              <div key={group.id}>
                {categoryFilter === 'all' ? (
                  <div className={styles.groupLabel}>
                    {t(meta?.labelKey ?? '', { defaultValue: meta?.defaultLabel })}
                  </div>
                ) : null}
                {group.looks.map((look) => {
                  const active = selectedLook?.id === look.id;
                  return (
                    <button
                      key={look.id}
                      type='button'
                      role='option'
                      aria-selected={active}
                      className={`${styles.item} ${active ? styles.itemActive : ''}`}
                      onClick={() => selectLook(look)}
                    >
                      <CanvasStyleCoverSwatch
                        cover={look.cover}
                        className={styles.cover}
                        alt={t('videoGeneration.looks.previewAlt', {
                          defaultValue: '{{title}}画风示意',
                          title: lookName(look, t),
                        })}
                      />
                      <span className={styles.itemLabel}>{lookName(look, t)}</span>
                      <span className={styles.checkSlot} aria-hidden='true'>
                        {active ? <CheckSmall theme='outline' size={14} fill='currentColor' /> : null}
                      </span>
                    </button>
                  );
                })}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}

export function LookField({
  stylePrompt,
  onSelect,
  allowEmpty = true,
  disabled,
}: LookPickerProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const look = lookByPrompt.get(stylePrompt.trim());
  const custom = visualStyleSelectValue(stylePrompt) === '__custom__';
  const label = look
    ? lookName(look, t)
    : custom
      ? t('videoGeneration.workspace.source.stylePresets.custom', { defaultValue: '自定义风格' })
      : t('videoGeneration.workspace.source.stylePlaceholder', { defaultValue: '选择视觉风格' });

  return (
    <ComposerAnchoredOverlay
      open={open && !disabled}
      onOpenChange={(next) => {
        if (!disabled) setOpen(next);
      }}
      width={400}
      estimatedHeight={520}
      padded={false}
      trigger={
        <button
          type='button'
          className={styles.fieldTrigger}
          disabled={disabled}
          aria-haspopup='listbox'
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
        >
          {look ? <CanvasStyleCoverSwatch cover={look.cover} className={styles.fieldCover} alt='' /> : null}
          <span className={`${styles.fieldLabel} ${look || custom ? '' : styles.fieldPlaceholder}`}>
            {label}
          </span>
        </button>
      }
    >
      <LookPicker
        stylePrompt={stylePrompt}
        allowEmpty={allowEmpty}
        disabled={disabled}
        onSelect={(next) => {
          onSelect(next);
          setOpen(false);
        }}
      />
    </ComposerAnchoredOverlay>
  );
}
