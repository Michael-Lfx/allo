export interface ComposerSkillChip {
  skillId: string;
  name: string;
  source: string;
  avatar?: string;
}

export function composerChipFromCatalog(
  skill: { skillId: string; name: string; source: string; avatar?: string | null },
  sourceLabel: string
): ComposerSkillChip {
  return {
    skillId: skill.skillId,
    name: skill.name,
    source: sourceLabel,
    ...(skill.avatar ? { avatar: skill.avatar } : {}),
  };
}
