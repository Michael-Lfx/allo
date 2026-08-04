/**
 * PresetPickerDrawer — a focused single-select preset picker.
 *
 * Explicit Skills are selected from the shared slash launcher in the composer;
 * preset bindings are resolved by the backend when a preset is chosen.
 */
import type { Preset, PresetReference } from '@/common/types/agent/presetTypes';
import type { TagFilterState } from '@/renderer/pages/settings/PresetSettings/presetUtils';

import { Drawer, Input } from '@arco-design/web-react';
import { Close, Search } from '@icon-park/react';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { usePresetTags } from '@/renderer/hooks/preset';
import PresetTagFilterBar from '@/renderer/pages/settings/PresetSettings/PresetTagFilterBar';
import { filterPresetsByTags } from '@/renderer/pages/settings/PresetSettings/presetUtils';
import DrawerPresetCard from './DrawerPresetCard';
import styles from '../index.module.css';

// ─── Props ────────────────────────────────────────────────────────────────────

export interface PresetPickerDrawerProps {
  visible: boolean;
  onClose: () => void;
  presets: Preset[];
  localeKey: string;
  // Preset single-select
  onSelectPreset: (presetId: PresetReference) => void;
  onFree: () => void;
}

// ─── Drawer width (responsive, mirrors PresetEditDrawer) ───────────────────

function computeDrawerWidth(): number {
  const viewportWidth = window.innerWidth || 1024;
  const targetWidth = Math.max(360, Math.floor(viewportWidth * 0.52));
  return Math.min(1024, targetWidth, Math.max(280, viewportWidth - 24));
}

const AVATAR_IMAGE_MAP: Record<string, string> = {};

// ─── Component ────────────────────────────────────────────────────────────────

const PresetPickerDrawer: React.FC<PresetPickerDrawerProps> = ({
  visible,
  onClose,
  presets,
  localeKey,
  onSelectPreset,
  onFree,
}) => {
  const { t } = useTranslation();
  const { audienceTags, scenarioTags, tagById } = usePresetTags();

  // ── Responsive width ──
  const [drawerWidth, setDrawerWidth] = useState(computeDrawerWidth);
  useEffect(() => {
    const handler = () => setDrawerWidth(computeDrawerWidth());
    window.addEventListener('resize', handler);
    return () => window.removeEventListener('resize', handler);
  }, []);

  // ── Local search + tag filter state ──
  const [query, setQuery] = useState('');
  const [tagFilter, setTagFilter] = useState<TagFilterState>({ audience: [], scenario: [] });

  // Reset local state when the drawer opens.
  useEffect(() => {
    if (visible) {
      setQuery('');
      setTagFilter({ audience: [], scenario: [] });
    }
  }, [visible]);

  // ── Filtered results ──
  const filteredPresets = useMemo(
    () =>
      // PresetListItem is a type alias for Preset — structurally identical
      filterPresetsByTags(presets, query, tagFilter, localeKey),
    [presets, query, tagFilter, localeKey]
  );

  // ── Handle preset select (single-select then close) ──
  const handleSelectPreset = useCallback(
    (id: PresetReference) => {
      onSelectPreset(id);
      onClose();
    },
    [onSelectPreset, onClose]
  );

  // ── Render ──
  return (
    <Drawer
      closable={false}
      visible={visible}
      placement="right"
      width={drawerWidth}
      zIndex={1200}
      getPopupContainer={() => document.body}
      autoFocus={false}
      onCancel={onClose}
      footer={null}
      headerStyle={{ display: 'none' }}
      bodyStyle={{ padding: 0, height: '100%' }}
    >
      <div className={styles.drawerSurface}>
        {/* Header */}
        <div className={styles.drawerTopbar}>
          <div className='text-14px font-semibold text-t-primary'>
            {t('guid.drawer.presetTab', { defaultValue: 'Presets' })}
          </div>

          {/* Close button */}
          <button
            type='button'
            className={styles.drawerCloseButton}
            onClick={onClose}
            aria-label={t('common.close', { defaultValue: 'Close' })}
          >
            <Close theme='outline' size={16} strokeWidth={3} />
          </button>
        </div>

        {/* Search */}
        <div className={styles.drawerSearchPanel}>
          <Input
            prefix={<Search theme='outline' size={15} />}
            placeholder={t('guid.drawer.searchPreset', { defaultValue: 'Search preset name or description...' })}
            value={query}
            onChange={setQuery}
            allowClear
            className={styles.drawerSearchInput}
          />
        </div>

        {/* Tag filter */}
        <div className={styles.drawerFilterPanel}>
          <PresetTagFilterBar
            audienceTags={audienceTags}
            scenarioTags={scenarioTags}
            value={tagFilter}
            onChange={setTagFilter}
            localeKey={localeKey}
            onManageTags={() => {/* Not in scope for this drawer */}}
            variant='drawer'
            hideManageTags
          />
        </div>

        {/* Result count */}
        <div className={styles.drawerResultMeta}>
          <span>
            <strong>{filteredPresets.length}</strong>
            {' '}
            {t('guid.drawer.presetCount', { defaultValue: 'presets' })}
          </span>
        </div>

        {/* Card list */}
        <div className={styles.drawerList}>
          {filteredPresets.length > 0 ? (
            filteredPresets.map((preset) => (
              <DrawerPresetCard
                key={preset.preset_id}
                preset={preset}
                selected={false}
                localeKey={localeKey}
                avatarImageMap={AVATAR_IMAGE_MAP}
                tagById={tagById}
                onSelect={handleSelectPreset}
              />
            ))
          ) : (
            <div className={styles.drawerEmptyState}>
              {presets.length === 0
                ? t('guid.drawer.presetEmpty', {
                    defaultValue: 'No presets yet. Create one in Presets, or stay free-form.',
                  })
                : t('guid.drawer.presetNoMatch', {
                    defaultValue: 'No presets match the current search and filters.',
                  })}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className={styles.drawerFooter}>
          <span className={styles.drawerFooterHint}>
            {t('guid.drawer.presetHint', { defaultValue: 'Selecting a preset freezes its configuration for this conversation.' })}
          </span>
          <button
            type='button'
            className={styles.drawerGhostButton}
            onClick={() => { onFree(); onClose(); }}
          >
            {t('guid.drawer.keepFree', { defaultValue: 'Keep freeform' })}
          </button>
        </div>
      </div>
    </Drawer>
  );
};

export default PresetPickerDrawer;
