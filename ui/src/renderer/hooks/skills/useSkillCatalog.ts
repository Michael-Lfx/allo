import { ipcBridge } from '@/common';
import { useCallback, useEffect, useState } from 'react';
import { resolveCatalogSkillAvatar } from '@/renderer/pages/settings/skill/skillPresentation';
import { SKILL_CATALOG_CHANGED_EVENT } from './skillCatalogEvents';

export type SkillCatalogSource = 'builtin' | 'user' | 'project' | 'extension' | 'mcp' | 'legacy' | 'capability';

export interface SkillCatalogEntry {
  skillId: string;
  name: string;
  description: string;
  source: SkillCatalogSource;
  sourceKey?: string;
  marketId?: string;
  nameI18n?: Record<string, string>;
  descriptionI18n?: Record<string, string>;
  avatar?: string | null;
}

function mapCatalogEntry(entry: {
  skill_id: string;
  name: string;
  description: string;
  source: SkillCatalogSource;
  source_key?: string;
  market_id?: string;
  name_i18n?: Record<string, string>;
  description_i18n?: Record<string, string>;
  avatar?: string | null;
}): SkillCatalogEntry {
  return {
    skillId: entry.skill_id,
    name: entry.name,
    description: entry.description,
    source: entry.source,
    sourceKey: entry.source_key,
    marketId: entry.market_id,
    nameI18n: entry.name_i18n,
    descriptionI18n: entry.description_i18n,
    avatar: resolveCatalogSkillAvatar(entry.name, entry.avatar, entry.source),
  };
}

/**
 * Composer and slash pickers list the Flowy skill library: skills the user
 * installed, plus shipped builtins and Flowy-managed capability Skills (the
 * backend lists those only while installed). A user copy of the same name replaces the
 * builtin. Skills only discovered from `~/.agents/skills` or a workspace
 * checkout are not library skills.
 */
export function isInstalledPickerSkill(
  skill: Pick<SkillCatalogEntry, 'source' | 'sourceKey'>,
): boolean {
  if (
    skill.source === 'builtin' ||
    skill.source === 'capability' ||
    skill.source === 'extension' ||
    skill.source === 'mcp'
  ) {
    return true;
  }
  return skill.source === 'user' && !skill.sourceKey;
}

export function installedPickerSkills<T extends Pick<SkillCatalogEntry, 'name' | 'source' | 'sourceKey'>>(
  skills: readonly T[],
): T[] {
  const installed = skills.filter(isInstalledPickerSkill);
  const copiedNames = new Set(
    installed.filter((skill) => skill.source !== 'builtin').map((skill) => skill.name),
  );
  return installed.filter((skill) => skill.source !== 'builtin' || !copiedNames.has(skill.name));
}

export function useSkillCatalog(enabled = true) {
  const [skills, setSkills] = useState<SkillCatalogEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(false);

  const refresh = useCallback(async () => {
    if (!enabled) {
      setSkills([]);
      setError(false);
      return;
    }
    setLoading(true);
    setError(false);
    try {
      const catalog = await ipcBridge.fs.listSkillCatalog.invoke();
      setSkills(catalog.skills.map(mapCatalogEntry));
    } catch (error) {
      console.warn('[skills] failed to refresh catalog', error);
      setSkills([]);
      setError(true);
    } finally {
      setLoading(false);
    }
  }, [enabled]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!enabled || typeof window === 'undefined') return;
    const handleCatalogChanged = () => {
      void refresh();
    };
    window.addEventListener(SKILL_CATALOG_CHANGED_EVENT, handleCatalogChanged);
    return () => window.removeEventListener(SKILL_CATALOG_CHANGED_EVENT, handleCatalogChanged);
  }, [enabled, refresh]);

  return { skills, loading, error, refresh };
}
