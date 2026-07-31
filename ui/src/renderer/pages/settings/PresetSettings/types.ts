import type { Preset } from '@/common/types/agent/presetTypes';

// Skill info type
export type SkillSource = 'builtin' | 'custom' | 'extension';

export type SkillInfo = {
  name: string;
  description: string;
  name_i18n?: Record<string, string>;
  description_i18n?: Record<string, string>;
  location: string;
  relative_location?: string;
  is_custom: boolean;
  source: SkillSource;
  // Skill side-store tag keys. Preset wire bindings use preset_tag_id.
  audience_tags?: string[];
  scenario_tags?: string[];
};

/**
 * Lightweight catalog metadata used only by Preset binding. Unlike the legacy
 * Skills Hub list, it deliberately retains builtin/user name collisions.
 */
export type PresetSkillCatalogSource = 'builtin' | 'user' | 'project' | 'extension' | 'mcp' | 'legacy';

export type PresetSkillCatalogItem = {
  skill_id: string;
  name: string;
  description: string;
  source: PresetSkillCatalogSource;
  source_key?: string;
};

// Pending skill to import
export type PendingSkill = {
  path: string;
  name: string;
  description: string;
  /** Canonical ID returned by the import endpoint, once available. */
  skillId?: string;
};

// Builtin auto-injected skill info
export type BuiltinAutoSkill = {
  name: string;
  description: string;
  name_i18n?: Record<string, string>;
  description_i18n?: Record<string, string>;
  location?: string;
};

export type PresetListItem = Preset;
