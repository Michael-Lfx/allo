import { useEffect, useMemo, useState } from 'react';
import { listVerticalSkills } from '../api';
import type { VerticalSkillSummary } from '../types';
import type { VideoHomeMode } from './types';

export interface VerticalSkillHubApi {
  skillCatalog: VerticalSkillSummary[];
  /** Merge a freshly fetched catalog into the cached one (hub emits per-open). */
  mergeCatalog: (list: VerticalSkillSummary[]) => void;
  reloadToken: number;
  bumpReloadToken: () => void;
  selectedVerticalSkills: ReadonlyArray<{ id: string; label: string }>;
}

/**
 * Agent-mode vertical Skill catalog: lazy load on mount / mode switch / create,
 * plus chip labels for the selected skill ids.
 */
export function useVerticalSkillHub(
  mode: VideoHomeMode,
  verticalSkillIds: string[]
): VerticalSkillHubApi {
  const [reloadToken, setReloadToken] = useState(0);
  const [skillCatalog, setSkillCatalog] = useState<VerticalSkillSummary[]>([]);

  useEffect(() => {
    if (mode !== 'agent') return;
    if (skillCatalog.length > 0 && reloadToken === 0) return;
    let cancelled = false;
    const loadCatalog = () => listVerticalSkills()
      .then((list) => {
        if (!cancelled) setSkillCatalog(list);
      })
      .catch(() => {
        /* catalog is best-effort for chip labels */
      });
    const idleWindow = window as Window & {
      requestIdleCallback?: (callback: () => void, options?: { timeout?: number }) => number;
      cancelIdleCallback?: (handle: number) => void;
    };
    if (typeof idleWindow.requestIdleCallback === 'function') {
      const idleId = idleWindow.requestIdleCallback(loadCatalog, { timeout: 1200 });
      return () => {
        cancelled = true;
        idleWindow.cancelIdleCallback?.(idleId);
      };
    }
    const timer = window.setTimeout(loadCatalog, 250);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [mode, skillCatalog.length, reloadToken]);

  const selectedVerticalSkills = useMemo(() => {
    const byId = new Map(skillCatalog.map((skill) => [skill.id, skill]));
    return verticalSkillIds.map((id) => {
      const skill = byId.get(id);
      return {
        id,
        label: skill?.display_name || skill?.name || id.replace(/^[^:]+:/, ''),
      };
    });
  }, [verticalSkillIds, skillCatalog]);

  const mergeCatalog = (list: VerticalSkillSummary[]) => {
    setSkillCatalog((prev) => {
      const map = new Map(prev.map((skill) => [skill.id, skill]));
      list.forEach((skill) => map.set(skill.id, skill));
      return Array.from(map.values());
    });
  };

  return {
    skillCatalog,
    mergeCatalog,
    reloadToken,
    bumpReloadToken: () => {
      setReloadToken((n) => n + 1);
    },
    selectedVerticalSkills,
  };
}
