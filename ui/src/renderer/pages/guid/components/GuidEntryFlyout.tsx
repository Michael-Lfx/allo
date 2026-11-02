/**
 * GuidEntryFlyout — homepage preset and Skill chooser beside the add menu.
 *
 * Preset selection still freezes one preset for the conversation. Skill
 * selection only toggles the current draft's chips and stays open.
 */
import type { Preset, PresetReference } from '@/common/types/agent/presetTypes';
import type { SkillCatalogEntry } from '@/renderer/hooks/skills/useSkillCatalog';
import { FlowyIcon } from '@renderer/components/base/FlowyIcon';
import FlowySearch from '@renderer/components/base/FlowySearch';
import PresetAvatar from '@/renderer/pages/settings/PresetSettings/PresetAvatar';
import SkillAvatar from '@/renderer/pages/settings/skill/SkillAvatar';
import { resolveSkillDisplay } from '@/renderer/pages/settings/skill/skillDisplay';
import React, { useEffect, useMemo, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import styles from '../index.module.css';
import { entryFlyoutPositionStyle, useEntryFlyoutBox } from './entryFlyoutPlacement';

export type GuidEntryFlyoutMode = 'preset' | 'skills';

export interface GuidEntryFlyoutProps {
  mode: GuidEntryFlyoutMode;
  anchorRef: React.RefObject<HTMLDivElement | null>;
  presets: Preset[];
  localeKey: string;
  selectedPresetId?: PresetReference;
  onSelectPreset: (presetId: PresetReference) => void;
  onFree: () => void;
  skills?: SkillCatalogEntry[];
  selectedSkillIds?: string[];
  onToggleSkill?: (skillId: string) => void;
  skillsLoading?: boolean;
  skillsError?: boolean;
  onManage: () => void;
}

export const GuidAnchoredFlyout = React.forwardRef<
  HTMLDivElement,
  {
    anchorRef: React.RefObject<HTMLDivElement | null>;
    label: string;
    testId?: string;
    children: React.ReactNode;
  }
>(function GuidAnchoredFlyout({ anchorRef, label, testId, children }, ref) {
  const box = useEntryFlyoutBox(anchorRef, true);

  if (typeof document === 'undefined') return null;

  return createPortal(
    <div
      ref={ref}
      className={`${styles.entryFlyout} ${styles.entryFlyoutEmbed}`}
      data-testid={testId}
      role='dialog'
      aria-label={label}
      style={{ ...entryFlyoutPositionStyle(box), overflowY: 'auto' }}
    >
      {children}
    </div>,
    document.body,
  );
});

const GuidEntryFlyout = React.forwardRef<HTMLDivElement, GuidEntryFlyoutProps>(function GuidEntryFlyout(
  {
    mode,
    anchorRef,
    presets,
    localeKey,
    selectedPresetId,
    onSelectPreset,
    onFree,
    skills = [],
    selectedSkillIds = [],
    onToggleSkill,
    skillsLoading = false,
    skillsError = false,
    onManage,
  },
  ref,
) {
  const { t, i18n } = useTranslation();
  const [query, setQuery] = useState('');
  const box = useEntryFlyoutBox(anchorRef, true);

  useEffect(() => {
    setQuery('');
  }, [mode]);

  const filteredPresets = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    if (!normalizedQuery) return presets;
    return presets.filter((preset) => {
      const name = preset.name_i18n?.[localeKey] || preset.name_i18n?.['en-US'] || preset.name;
      const description =
        preset.description_i18n?.[localeKey] || preset.description_i18n?.['en-US'] || preset.description || '';
      return `${name} ${description}`.toLocaleLowerCase().includes(normalizedQuery);
    });
  }, [localeKey, presets, query]);

  const filteredSkills = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    if (!normalizedQuery) return skills;
    return skills.filter((skill) => {
      const display = resolveSkillDisplay(
        {
          name: skill.name,
          description: skill.description,
          name_i18n: skill.nameI18n,
          description_i18n: skill.descriptionI18n,
        },
        i18n.language || localeKey,
      );
      return `${display.name} ${display.description} ${skill.source}`.toLocaleLowerCase().includes(normalizedQuery);
    });
  }, [i18n.language, localeKey, query, skills]);

  const selectedSkillIdSet = useMemo(() => new Set(selectedSkillIds), [selectedSkillIds]);
  const manageLabel =
    mode === 'preset'
      ? t('guid.flyout.managePreset', { defaultValue: '管理设定' })
      : t('guid.flyout.manageSkills', { defaultValue: '管理技能' });

  const list = (() => {
    if (mode === 'preset') {
      if (filteredPresets.length === 0) {
        return (
          <div className={styles.entryFlyoutEmpty}>
            {presets.length === 0
              ? t('guid.drawer.presetEmpty', {
                  defaultValue: '还没有设定。可前往设定页创建，或保持自由发挥。',
                })
              : t('guid.flyout.presetNoMatch', {
                  defaultValue: '没有匹配当前搜索的设定。',
                })}
          </div>
        );
      }
      return filteredPresets.map((preset) => {
        const name = preset.name_i18n?.[localeKey] || preset.name_i18n?.['en-US'] || preset.name;
        const description =
          preset.description_i18n?.[localeKey] || preset.description_i18n?.['en-US'] || preset.description || '';
        const selected = preset.preset_id === selectedPresetId;
        return (
          <button
            key={preset.preset_id}
            type='button'
            className={styles.entryFlyoutRow}
            aria-pressed={selected}
            data-testid={`guid-entry-flyout-preset-${preset.preset_id}`}
            onClick={() => onSelectPreset(preset.preset_id)}
          >
            <span className={styles.entryFlyoutIcon}>
              <PresetAvatar preset={preset} size={28} avatarImageMap={{}} />
            </span>
            <span className={styles.entryFlyoutBody}>
              <span className={styles.entryFlyoutTitle}>{name}</span>
              {description ? <span className={styles.entryFlyoutDesc}>{description}</span> : null}
            </span>
            <span className={styles.entryFlyoutMark} aria-hidden='true'>
              {selected ? <FlowyIcon name='check' size={14} /> : null}
            </span>
          </button>
        );
      });
    }

    if (skillsLoading) {
      return (
        <div className={styles.entryFlyoutEmpty} aria-busy='true'>
          {t('common.loading', { defaultValue: '加载中...' })}
        </div>
      );
    }
    if (skillsError) {
      return (
        <div className={styles.entryFlyoutEmpty} role='alert'>
          {t('guid.drawer.skillLoadFailed', { defaultValue: 'Skills 暂时无法加载，请稍后重试。' })}
        </div>
      );
    }
    if (filteredSkills.length === 0) {
      return (
        <div className={styles.entryFlyoutEmpty}>
          {skills.length === 0
            ? t('guid.drawer.skillEmpty', { defaultValue: '还没有已安装的技能。' })
            : t('guid.drawer.skillNoMatch', { defaultValue: '没有匹配当前搜索条件的 Skill。' })}
        </div>
      );
    }
    return filteredSkills.map((skill) => {
      const display = resolveSkillDisplay(
        {
          name: skill.name,
          description: skill.description,
          name_i18n: skill.nameI18n,
          description_i18n: skill.descriptionI18n,
        },
        i18n.language || localeKey,
      );
      const selected = selectedSkillIdSet.has(skill.skillId);
      return (
        <button
          key={skill.skillId}
          type='button'
          className={styles.entryFlyoutRow}
          aria-pressed={selected}
          data-testid={`guid-entry-flyout-skill-${skill.skillId}`}
          onClick={() => onToggleSkill?.(skill.skillId)}
        >
          <span className={styles.entryFlyoutIcon}>
            <SkillAvatar
              skill={{ name: skill.name, avatar: skill.avatar }}
              size={28}
              radiusClassName='rounded-8px'
              showShadow={false}
            />
          </span>
          <span className={styles.entryFlyoutBody}>
            <span className={styles.entryFlyoutTitle}>{display.name}</span>
            <span className={styles.entryFlyoutDesc}>
              {display.description || t('guid.drawer.skillNoDescription', { defaultValue: '暂无描述。' })}
            </span>
          </span>
          <span className={styles.entryFlyoutMark} aria-hidden='true'>
            {selected ? <FlowyIcon name='check' size={14} /> : null}
          </span>
        </button>
      );
    });
  })();

  if (typeof document === 'undefined') return null;

  return createPortal(
    <div
      ref={ref}
      className={styles.entryFlyout}
      data-testid='guid-entry-flyout'
      data-mode={mode}
      role='dialog'
      aria-label={
        mode === 'preset'
          ? t('guid.drawer.presetTab', { defaultValue: '设定' })
          : t('guid.drawer.skillsTab', { defaultValue: 'Skills' })
      }
      style={entryFlyoutPositionStyle(box)}
    >
      <button
        type='button'
        className={styles.entryFlyoutAction}
        data-testid='guid-entry-flyout-manage'
        onClick={onManage}
      >
        <FlowyIcon name='settings' size={16} />
        <span className={styles.entryFlyoutActionLabel}>{manageLabel}</span>
        <FlowyIcon name='right' size={14} />
      </button>
      {mode === 'preset' ? (
        <button
          type='button'
          className={styles.entryFlyoutAction}
          data-testid='guid-entry-flyout-free'
          aria-pressed={!selectedPresetId}
          onClick={onFree}
        >
          <span className={styles.entryFlyoutActionLabel}>
            {t('guid.drawer.keepFree', { defaultValue: '保持自由发挥' })}
          </span>
          <span className={styles.entryFlyoutMark} aria-hidden='true'>
            {!selectedPresetId ? <FlowyIcon name='check' size={14} /> : null}
          </span>
        </button>
      ) : null}
      <div className={styles.entryFlyoutList}>{list}</div>
      <div className={styles.entryFlyoutSearch}>
        <FlowySearch
          density='compact'
          value={query}
          onChange={setQuery}
          placeholder={t(mode === 'preset' ? 'guid.drawer.searchPreset' : 'guid.drawer.searchSkill', {
            defaultValue: mode === 'preset' ? '搜索设定名称或描述…' : '搜索 Skill…',
          })}
        />
      </div>
    </div>,
    document.body,
  );
});

export default GuidEntryFlyout;
