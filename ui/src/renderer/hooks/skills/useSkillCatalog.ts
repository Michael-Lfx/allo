import { ipcBridge } from '@/common';
import { useCallback, useEffect, useState } from 'react';
import { SKILL_CATALOG_CHANGED_EVENT } from './skillCatalogEvents';

export type SkillCatalogSource = 'builtin' | 'user' | 'project' | 'extension' | 'mcp' | 'legacy';

export interface SkillCatalogEntry {
  skillId: string;
  name: string;
  description: string;
  source: SkillCatalogSource;
  sourceKey?: string;
  marketId?: string;
}

function mapCatalogEntry(entry: {
  skill_id: string;
  name: string;
  description: string;
  source: SkillCatalogSource;
  source_key?: string;
  market_id?: string;
}): SkillCatalogEntry {
  return {
    skillId: entry.skill_id,
    name: entry.name,
    description: entry.description,
    source: entry.source,
    sourceKey: entry.source_key,
    marketId: entry.market_id,
  };
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
