import type { PendingSkill, PresetSkillCatalogItem } from './types';

const PENDING_SKILL_PREFIX = 'pending:';

export type SelectedPresetSkill = {
  skillId: string;
  name: string;
};

export type PresetSkillSaveSelection = {
  skillIds: string[];
  unresolvedPendingSkillNames: string[];
};

export const pendingSkillSelectionId = (skill: Pick<PendingSkill, 'path'>): string =>
  `${PENDING_SKILL_PREFIX}${encodeURIComponent(skill.path)}`;

export const mergePresetSkillIds = (current: string[], additions: string[]): string[] => {
  const seen = new Set<string>();
  return [...current, ...additions].filter((skillId) => {
    if (seen.has(skillId)) return false;
    seen.add(skillId);
    return true;
  });
};

export const uniqueCatalogUserSkillIdForName = (
  catalog: PresetSkillCatalogItem[],
  name: string,
): string | undefined => {
  const matches = catalog.filter((skill) => skill.source === 'user' && skill.name === name);
  return matches.length === 1 ? matches[0].skill_id : undefined;
};

/**
 * Pending imported Skills retain the canonical ID returned by their import
 * operation. Old pending items without that ID may use a catalog name only
 * when it is unique; ambiguous names are never expanded into multiple IDs.
 */
export const resolvePresetSkillIdsForSave = (
  selectedSkillIds: string[],
  pendingSkills: PendingSkill[],
  catalog: PresetSkillCatalogItem[],
): PresetSkillSaveSelection => {
  const pendingBySelectionId = new Map(
    pendingSkills.map((skill) => [pendingSkillSelectionId(skill), skill]),
  );
  const explicitSkillIds = selectedSkillIds.filter(
    (skillId) => !pendingBySelectionId.has(skillId),
  );
  const selectedPendingSkills = selectedSkillIds
    .map((skillId) => pendingBySelectionId.get(skillId))
    .filter((skill): skill is PendingSkill => Boolean(skill));
  const resolvedPendingIds = selectedPendingSkills.flatMap((skill) => {
    if (skill.skillId) return [skill.skillId];
    const skillId = uniqueCatalogUserSkillIdForName(catalog, skill.name);
    return skillId ? [skillId] : [];
  });
  const unresolvedPendingSkillNames = selectedPendingSkills
    .filter((skill) => !skill.skillId && !uniqueCatalogUserSkillIdForName(catalog, skill.name))
    .map((skill) => skill.name);

  return {
    skillIds: mergePresetSkillIds(explicitSkillIds, resolvedPendingIds),
    unresolvedPendingSkillNames,
  };
};
