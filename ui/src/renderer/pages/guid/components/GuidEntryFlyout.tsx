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
import { WORKSPACE_PICKER_POPOVER_Z_INDEX } from '@/renderer/components/workspace/WorkspacePickerPopover';
import PresetAvatar from '@/renderer/pages/settings/PresetSettings/PresetAvatar';
import SkillAvatar from '@/renderer/pages/settings/skill/SkillAvatar';
import { resolveSkillDisplay } from '@/renderer/pages/settings/skill/skillDisplay';
import React, { useEffect, useLayoutEffect, useMemo, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import styles from '../index.module.css';

export type GuidEntryFlyoutMode = 'preset' | 'skills';

const FLYOUT_WIDTH = 320;
const FLYOUT_GAP = 0;
const FLYOUT_MARGIN = 8;
const FLYOUT_MAX_HEIGHT = 440;

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

type FlyoutBox = {
  left: number;
  top: number;
  width: number;
  maxHeight: number;
};

/** Top of the open primary row, so the submenu's top edge lines up with it. */
export const entryFlyoutAlignTop = (root: ParentNode | null, fallback: number): number => {
  const row = root?.querySelector<HTMLElement>('[role="option"][aria-selected="true"]');
  return row?.getBoundingClientRect().top ?? fallback;
};

export const measureEntryFlyout = (anchor: DOMRect, alignTop = anchor.top): FlyoutBox => {
  const viewportWidth = window.innerWidth || document.documentElement.clientWidth;
  const viewportHeight = window.innerHeight || document.documentElement.clientHeight;
  const width = Math.min(FLYOUT_WIDTH, Math.max(0, viewportWidth - FLYOUT_MARGIN * 2));
  const spaceRight = viewportWidth - anchor.right - FLYOUT_GAP - FLYOUT_MARGIN;
  const spaceLeft = anchor.left - FLYOUT_GAP - FLYOUT_MARGIN;
  const placeRight = spaceRight >= width || spaceRight >= spaceLeft;
  const left = placeRight
    ? Math.min(Math.max(FLYOUT_MARGIN, anchor.right + FLYOUT_GAP), Math.max(FLYOUT_MARGIN, viewportWidth - width - FLYOUT_MARGIN))
    : Math.max(FLYOUT_MARGIN, anchor.left - FLYOUT_GAP - width);
  const top = Math.max(FLYOUT_MARGIN, Math.min(alignTop, viewportHeight - FLYOUT_MARGIN - 48));
  const maxHeight = Math.min(FLYOUT_MAX_HEIGHT, Math.max(48, viewportHeight - top - FLYOUT_MARGIN));
  return { left, top, width, maxHeight };
};

export const GuidAnchoredFlyout = React.forwardRef<
  HTMLDivElement,
  {
    anchorRef: React.RefObject<HTMLDivElement | null>;
    label: string;
    testId?: string;
    children: React.ReactNode;
  }
>(function GuidAnchoredFlyout({ anchorRef, label, testId, children }, ref) {
  const [box, setBox] = useState<FlyoutBox | null>(null);

  useLayoutEffect(() => {
    const update = () => {
      const anchor = anchorRef.current;
      if (!anchor) return;
      const rect = anchor.getBoundingClientRect();
      setBox(measureEntryFlyout(rect, entryFlyoutAlignTop(anchor, rect.top)));
    };
    update();
    window.addEventListener('resize', update);
    window.addEventListener('scroll', update, true);
    return () => {
      window.removeEventListener('resize', update);
      window.removeEventListener('scroll', update, true);
    };
  }, [anchorRef]);

  if (typeof document === 'undefined') return null;

  return createPortal(
    <div
      ref={ref}
      className={`${styles.entryFlyout} ${styles.entryFlyoutEmbed}`}
      data-testid={testId}
      role='dialog'
      aria-label={label}
      style={{
        position: 'fixed',
        left: box?.left ?? -9999,
        top: box?.top ?? FLYOUT_MARGIN,
        width: box?.width ?? FLYOUT_WIDTH,
        maxHeight: box?.maxHeight ?? FLYOUT_MAX_HEIGHT,
        overflowY: 'auto',
        visibility: box ? 'visible' : 'hidden',
        zIndex: WORKSPACE_PICKER_POPOVER_Z_INDEX,
      }}
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
  const [box, setBox] = useState<FlyoutBox | null>(null);

  useEffect(() => {
    setQuery('');
  }, [mode]);

  useLayoutEffect(() => {
    const update = () => {
      const anchor = anchorRef.current;
      if (!anchor) return;
      const rect = anchor.getBoundingClientRect();
      setBox(measureEntryFlyout(rect, entryFlyoutAlignTop(anchor, rect.top)));
    };
    update();
    window.addEventListener('resize', update);
    window.addEventListener('scroll', update, true);
    return () => {
      window.removeEventListener('resize', update);
      window.removeEventListener('scroll', update, true);
    };
  }, [anchorRef, mode]);

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
            ? t('guid.drawer.skillEmpty', { defaultValue: '当前运行环境没有可用的 Skill。' })
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
      style={{
        position: 'fixed',
        left: box?.left ?? -9999,
        top: box?.top ?? FLYOUT_MARGIN,
        width: box?.width ?? FLYOUT_WIDTH,
        maxHeight: box?.maxHeight ?? FLYOUT_MAX_HEIGHT,
        visibility: box ? 'visible' : 'hidden',
        zIndex: WORKSPACE_PICKER_POPOVER_Z_INDEX,
      }}
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
