/**
 * Installed-skill tile. Uses the same ExpertTile as the skill market so
 * "My skills" and the catalog share one card.
 */
import React from 'react';
import { useTranslation } from 'react-i18next';
import { getBaseUrl } from '@/common/adapter/httpBridge';
import type { PresetTag } from '@/common/types/agent/presetTypes';
import ExpertTile from '@/renderer/pages/settings/PresetSettings/ExpertTile';
import type { SkillInfo } from '@/renderer/pages/settings/PresetSettings/types';
import { resolveSkillDisplay } from './skillDisplay';
import { normalizeTestId, resolveSkillAvatarSrc } from './skillPresentation';

type SkillCardProps = {
  skill: SkillInfo;
  /** Shared preset tag vocabulary, for resolving tag keys → labels. */
  tagByKey: Map<string, PresetTag>;
  localeKey: string;
  /** True when the skill name is in the built-in auto-inject set (parent-supplied). */
  isAutoInjected: boolean;
  onOpenDetails: (skill: SkillInfo) => void;
  onEditTags: (skill: SkillInfo) => void;
  highlighted?: boolean;
  cardRef?: (el: HTMLDivElement | null) => void;
};

const SkillCard: React.FC<SkillCardProps> = ({
  skill,
  tagByKey,
  localeKey,
  isAutoInjected,
  onOpenDetails,
  onEditTags,
  highlighted = false,
  cardRef,
}) => {
  const { t } = useTranslation();
  const testId = normalizeTestId(skill.name);
  const display = resolveSkillDisplay(skill, localeKey);
  const sourceLabel = isAutoInjected
    ? t('settings.skillsHub.sourceAuto', { defaultValue: 'Auto' })
    : skill.source === 'custom'
      ? t('settings.skillsHub.custom', { defaultValue: 'Custom' })
      : skill.source === 'extension'
        ? t('settings.skillsHub.sourceExtension', { defaultValue: 'Extension' })
        : t('settings.skillsHub.builtin', { defaultValue: 'Built-in' });
  const tagLabels = [
    sourceLabel,
    ...[...(skill.audience_tags ?? []), ...(skill.scenario_tags ?? [])]
      .map((key) => tagByKey.get(key))
      .filter((tag): tag is PresetTag => Boolean(tag))
      .map((tag) => tag.label_i18n?.[localeKey] || tag.label),
  ];

  return (
    <ExpertTile
      rootRef={cardRef}
      testId={`skill-card-${testId}`}
      name={display.name}
      avatarUrl={resolveSkillAvatarSrc(skill.avatar, getBaseUrl())}
      summary={display.description || t('settings.skillsHub.noDescription', { defaultValue: 'No description provided.' })}
      tags={tagLabels}
      actionLabel={t('settings.skillsHub.editTags', { defaultValue: 'Edit Tags' })}
      actionTestId={`btn-edit-tags-${testId}`}
      onOpen={() => onOpenDetails(skill)}
      onAction={() => onEditTags(skill)}
      className={highlighted ? 'ring-2 ring-[rgba(var(--primary-6),0.45)]' : undefined}
    />
  );
};

export default SkillCard;
