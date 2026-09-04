/**
 * DrawerPresetCard — Single-select preset card for PresetPickerDrawer.
 * Displays avatar, name, description, engine/model capsule, and tag chips. Only
 * catalog-owned builtin/extension presets receive a source badge; user presets,
 * including historical expert-package installs, use the local preset shape.
 */
import type { Preset, PresetReference, PresetTag } from '@/common/types/agent/presetTypes';
import React from 'react';
import { useTranslation } from 'react-i18next';
import PresetAvatar from '@/renderer/pages/settings/PresetSettings/PresetAvatar';
import { CheckSmall } from '@icon-park/react';
import styles from '../index.module.css';

export type DrawerPresetCardProps = {
  preset: Preset;
  selected: boolean;
  localeKey: string;
  avatarImageMap: Record<string, string>;
  tagById: Map<string, PresetTag>;
  onSelect: (presetId: PresetReference) => void;
};

const DrawerPresetCard: React.FC<DrawerPresetCardProps> = ({
  preset,
  selected,
  localeKey,
  avatarImageMap,
  tagById,
  onSelect,
}) => {
  const { t } = useTranslation();
  const name = preset.name_i18n?.[localeKey] || preset.name_i18n?.['en-US'] || preset.name;
  const description =
    preset.description_i18n?.[localeKey] || preset.description_i18n?.['en-US'] || preset.description || '';
  // User presets include packages that were downloaded before the package
  // market was retired. They are intentionally rendered as local presets:
  // no market/source badge or remote detail affordance is exposed here.
  const sourceLabel =
    preset.source === 'extension'
      ? t('guid.drawer.sourceExtension', { defaultValue: '扩展' })
      : preset.source === 'builtin'
        ? t('guid.drawer.sourceBuiltin', { defaultValue: '内置' })
        : null;
  const visibleTags = [...preset.audience_tag_ids, ...preset.scenario_tag_ids]
    .map((presetTagId) => tagById.get(presetTagId))
    .filter((tag): tag is PresetTag => Boolean(tag))
    .slice(0, 4);
  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      onSelect(preset.preset_id);
    }
  };

  return (
    <div
      role='button'
      tabIndex={0}
      className={[
        styles.drawerCard,
        selected ? styles.drawerCardSelected : '',
      ].filter(Boolean).join(' ')}
      onClick={() => onSelect(preset.preset_id)}
      onKeyDown={handleKeyDown}
    >
      {/* Radio indicator */}
      <span
        className={[
          styles.drawerCardStatus,
          selected ? styles.drawerCardStatusSelected : '',
        ].filter(Boolean).join(' ')}
        aria-hidden='true'
      >
        {selected && <CheckSmall theme='filled' size={12} fill='currentColor' />}
      </span>

      {/* Avatar */}
      <div className={styles.drawerIconTile}>
        <PresetAvatar preset={preset} size={40} avatarImageMap={avatarImageMap} />
      </div>

      {/* Body */}
      <div className={styles.drawerCardBody}>
        <div className={styles.drawerCardTitleRow}>
          <h4 className={styles.drawerCardTitle}>{name}</h4>
          {sourceLabel ? (
            <span className={[styles.drawerBadge, styles.drawerBadgeMuted].join(' ')}>{sourceLabel}</span>
          ) : null}
        </div>

        <p className={styles.drawerDescription}>{description}</p>

        {/* Meta row: engine capsule + tag chips */}
        <div className={styles.drawerMetaRow}>
          {preset.model_preferences[0] && (
            <span className={styles.drawerEngineBadge}>
              <span className={styles.drawerEngineGlyph}>
                {(preset.preferred_agent_id || preset.agent_preferences[0]?.agent_id)?.[0]?.toUpperCase() || '◆'}
              </span>
              {preset.model_preferences[0].model}
            </span>
          )}
          {visibleTags.map((tag) => (
            <span key={tag.preset_tag_id} className={styles.drawerTagChip}>
              {tag.label_i18n?.[localeKey] || tag.label}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
};

export default DrawerPresetCard;
