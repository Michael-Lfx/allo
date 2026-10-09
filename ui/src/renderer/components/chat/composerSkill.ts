import { resolveSkillDisplay } from '@/renderer/pages/settings/skill/skillDisplay';

export interface ComposerSkillChip {
  skillId: string;
  name: string;
  source: string;
  avatar?: string;
}

export function composerChipFromCatalog(
  skill: {
    skillId: string;
    name: string;
    source: string;
    avatar?: string | null;
    nameI18n?: Record<string, string>;
  },
  sourceLabel: string,
  localeKey: string
): ComposerSkillChip {
  return {
    skillId: skill.skillId,
    name: resolveSkillDisplay({ name: skill.name, name_i18n: skill.nameI18n }, localeKey).name,
    source: sourceLabel,
    ...(skill.avatar ? { avatar: skill.avatar } : {}),
  };
}
