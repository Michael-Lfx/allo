/**
 * PresetPickerDrawer — the shared home entry selector.
 *
 * Presets and explicit Skills are two modes over the same draft-owned entry
 * surface. Preset selection freezes a preset reference for the conversation;
 * Skill selection only updates the current draft's source-qualified chips.
 */
import type { Preset, PresetReference } from '@/common/types/agent/presetTypes';
import type { SkillCatalogEntry } from '@/renderer/hooks/skills/useSkillCatalog';
import type { TagFilterState } from '@/renderer/pages/settings/PresetSettings/presetUtils';

import { Drawer, Input } from '@arco-design/web-react';
import { Close, Search } from '@icon-park/react';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { usePresetTags } from '@/renderer/hooks/preset';
import PresetTagFilterBar from '@/renderer/pages/settings/PresetSettings/PresetTagFilterBar';
import { filterPresetsByTags } from '@/renderer/pages/settings/PresetSettings/presetUtils';
import DrawerPresetCard from './DrawerPresetCard';
import DrawerSkillCard from './DrawerSkillCard';
import styles from '../index.module.css';

export type PresetPickerDrawerMode = 'preset' | 'skills';

export interface PresetPickerDrawerProps {
  visible: boolean;
  onClose: () => void;
  presets: Preset[];
  localeKey: string;
  mode: PresetPickerDrawerMode;
  onModeChange: (mode: PresetPickerDrawerMode) => void;
  selectedPresetId?: PresetReference;
  skills?: SkillCatalogEntry[];
  selectedSkillIds?: string[];
  onToggleSkill?: (skillId: string) => void;
  skillsLoading?: boolean;
  skillsError?: boolean;
  // Preset single-select
  onSelectPreset: (presetId: PresetReference) => void;
  onFree: () => void;
}

function computeDrawerWidth(): number {
  const viewportWidth = window.innerWidth || 1024;
  const targetWidth = Math.max(360, Math.floor(viewportWidth * 0.52));
  return Math.min(1024, targetWidth, Math.max(280, viewportWidth - 24));
}

const AVATAR_IMAGE_MAP: Record<string, string> = {};

const PresetPickerDrawer: React.FC<PresetPickerDrawerProps> = ({
  visible,
  onClose,
  presets,
  localeKey,
  mode,
  onModeChange,
  selectedPresetId,
  skills = [],
  selectedSkillIds = [],
  onToggleSkill,
  skillsLoading = false,
  skillsError = false,
  onSelectPreset,
  onFree,
}) => {
  const { t } = useTranslation();
  const { audienceTags, scenarioTags, tagById } = usePresetTags();
  const [drawerWidth, setDrawerWidth] = useState(computeDrawerWidth);
  const [query, setQuery] = useState('');
  const [tagFilter, setTagFilter] = useState<TagFilterState>({ audience: [], scenario: [] });

  useEffect(() => {
    const handler = () => setDrawerWidth(computeDrawerWidth());
    window.addEventListener('resize', handler);
    return () => window.removeEventListener('resize', handler);
  }, []);

  useEffect(() => {
    if (!visible) return;
    setQuery('');
    setTagFilter({ audience: [], scenario: [] });
  }, [visible, mode]);

  const filteredPresets = useMemo(
    () => filterPresetsByTags(presets, query, tagFilter, localeKey),
    [localeKey, presets, query, tagFilter],
  );

  const filteredSkills = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    if (!normalizedQuery) return skills;
    return skills.filter((skill) =>
      [skill.name, skill.description, skill.source].join(' ').toLocaleLowerCase().includes(normalizedQuery),
    );
  }, [query, skills]);

  const handleSelectPreset = useCallback(
    (id: PresetReference) => {
      onSelectPreset(id);
      onClose();
    },
    [onClose, onSelectPreset],
  );

  const selectedSkillIdSet = useMemo(() => new Set(selectedSkillIds), [selectedSkillIds]);
  const visibleCount = mode === 'preset' ? filteredPresets.length : filteredSkills.length;

  return (
    <Drawer
      closable={false}
      visible={visible}
      placement='right'
      width={drawerWidth}
      zIndex={1200}
      getPopupContainer={() => document.body}
      autoFocus={false}
      onCancel={onClose}
      footer={null}
      headerStyle={{ display: 'none' }}
      bodyStyle={{ padding: 0, height: '100%' }}
    >
      <div className={styles.drawerSurface} data-testid='guid-entry-drawer'>
        <div className={styles.drawerTopbar}>
          <div className={styles.drawerSegmented} role='tablist' aria-label={t('guid.drawer.modeLabel', { defaultValue: 'Entry type' })}>
            <button
              type='button'
              role='tab'
              aria-selected={mode === 'preset'}
              className={[styles.drawerSegment, mode === 'preset' ? styles.drawerSegmentActive : ''].filter(Boolean).join(' ')}
              onClick={() => onModeChange('preset')}
            >
              {t('guid.drawer.presetTab', { defaultValue: 'Presets' })}
            </button>
            <button
              type='button'
              role='tab'
              aria-selected={mode === 'skills'}
              className={[styles.drawerSegment, mode === 'skills' ? styles.drawerSegmentActive : ''].filter(Boolean).join(' ')}
              onClick={() => onModeChange('skills')}
            >
              {t('guid.drawer.skillsTab', { defaultValue: 'Skills' })}
            </button>
          </div>
          <button
            type='button'
            className={styles.drawerCloseButton}
            onClick={onClose}
            aria-label={t('common.close', { defaultValue: 'Close' })}
          >
            <Close theme='outline' size={16} strokeWidth={3} />
          </button>
        </div>

        <div className={styles.drawerSearchPanel}>
          <Input
            prefix={<Search theme='outline' size={15} />}
            placeholder={t(
              mode === 'preset' ? 'guid.drawer.searchPreset' : 'guid.drawer.searchSkill',
              { defaultValue: mode === 'preset' ? 'Search preset name or description...' : 'Search Skills...' },
            )}
            value={query}
            onChange={setQuery}
            allowClear
            className={styles.drawerSearchInput}
          />
        </div>

        {mode === 'preset' && (
          <div className={styles.drawerFilterPanel}>
            <PresetTagFilterBar
              audienceTags={audienceTags}
              scenarioTags={scenarioTags}
              value={tagFilter}
              onChange={setTagFilter}
              localeKey={localeKey}
              onManageTags={() => undefined}
              variant='drawer'
              hideManageTags
            />
          </div>
        )}

        <div className={styles.drawerResultMeta}>
          <span>
            <strong>{visibleCount}</strong>{' '}
            {t(mode === 'preset' ? 'guid.drawer.presetCount' : 'guid.drawer.skillCount', {
              defaultValue: mode === 'preset' ? 'presets' : 'Skills',
            })}
          </span>
          {mode === 'skills' && selectedSkillIds.length > 0 && (
            <span className={styles.entrySkillPopoverCount}>
              {t('guid.drawer.selectedSkillCount', { count: selectedSkillIds.length, defaultValue: '{{count}} selected' })}
            </span>
          )}
        </div>

        <div className={styles.drawerList}>
          {mode === 'preset' ? (
            filteredPresets.length > 0 ? (
              filteredPresets.map((preset) => (
                <DrawerPresetCard
                  key={preset.preset_id}
                  preset={preset}
                  selected={preset.preset_id === selectedPresetId}
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
            )
          ) : skillsLoading ? (
            <div className={styles.drawerEmptyState} aria-busy='true'>
              {t('common.loading', { defaultValue: '加载中...' })}
            </div>
          ) : skillsError ? (
            <div className={styles.drawerEmptyState} role='alert'>
              {t('guid.drawer.skillLoadFailed', { defaultValue: 'Skills 暂时无法加载，请稍后重试。' })}
            </div>
          ) : filteredSkills.length > 0 ? (
            filteredSkills.map((skill) => (
              <DrawerSkillCard
                key={skill.skillId}
                skill={skill}
                selected={selectedSkillIdSet.has(skill.skillId)}
                onToggle={(skillId) => onToggleSkill?.(skillId)}
              />
            ))
          ) : (
            <div className={styles.drawerEmptyState}>
              {skills.length === 0
                ? t('guid.drawer.skillEmpty', { defaultValue: 'No Skills are available for this runtime.' })
                : t('guid.drawer.skillNoMatch', { defaultValue: 'No Skills match the current search.' })}
            </div>
          )}
        </div>

        <div className={styles.drawerFooter}>
          {mode === 'preset' ? (
            <>
              <span className={styles.drawerFooterHint}>
                {t('guid.drawer.presetHint', { defaultValue: 'Selecting a preset freezes its configuration for this conversation.' })}
              </span>
              <button
                type='button'
                className={styles.drawerGhostButton}
                onClick={() => {
                  onFree();
                  onClose();
                }}
              >
                {t('guid.drawer.keepFree', { defaultValue: 'Keep freeform' })}
              </button>
            </>
          ) : (
            <>
              <span className={styles.drawerFooterHint}>
                {t('guid.drawer.skillHint', { defaultValue: 'Selected Skills apply to this conversation only.' })}
              </span>
              <button type='button' className={styles.drawerPrimaryButton} onClick={onClose}>
                {t('common.done', { defaultValue: '完成' })}
              </button>
            </>
          )}
        </div>
      </div>
    </Drawer>
  );
};

export default PresetPickerDrawer;
