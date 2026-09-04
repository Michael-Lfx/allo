/**
 * Cross-mode "最近创作" — agent sessions, clip tasks, canvas projects, briefings.
 * Home gallery and the sider strip share this merge so switching the composer
 * mode never hides another mode's work.
 */

import type { RecentVideoGenerationEntry } from './routeMemory';

export type RecentCreationKind = 'session' | 'task' | 'canvas' | 'briefing';

export type RecentNavItem = {
  id: string;
  title: string;
  status?: string | null;
  source: RecentCreationKind;
};

type CatalogHit = {
  title: string;
  status: string | null;
  source: RecentCreationKind;
  updatedAt: number;
};

/** Epoch ms. Seconds (< 1e12) are scaled; RFC3339 strings are parsed. */
export function toUpdatedAtMs(value: string | number | null | undefined): number {
  if (value == null) return 0;
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value > 0 && value < 1e12 ? value * 1000 : value;
  }
  const parsed = Date.parse(String(value));
  return Number.isNaN(parsed) ? 0 : parsed;
}

export type SessionLike = {
  id: string;
  title?: string | null;
  status?: string | null;
  updated_at?: number | string | null;
};

export type TaskLike = {
  task_id: string;
  prompt?: string | null;
  status?: string | null;
  updated_at?: number | string | null;
};

export type CanvasLike = {
  project_id: string;
  title?: string | null;
  updated_at?: number | string | null;
};

export type BriefingLike = {
  id: string;
  title?: string | null;
  status?: string | null;
  updated_at?: number | string | null;
};

function taskTitleFromPrompt(prompt: string | null | undefined): string {
  return (prompt ?? '').trim().slice(0, 48);
}

function navSource(source: RecentVideoGenerationEntry['source']): RecentCreationKind {
  return source === 'task' || source === 'canvas' || source === 'briefing' ? source : 'session';
}

function catalogFrom(
  sessions: SessionLike[],
  tasks: TaskLike[],
  canvases: CanvasLike[],
  briefings: BriefingLike[]
): Map<string, CatalogHit> {
  const catalog = new Map<string, CatalogHit>();
  for (const s of sessions) {
    catalog.set(s.id, {
      title: (s.title ?? '').trim(),
      status: s.status ?? null,
      source: 'session',
      updatedAt: toUpdatedAtMs(s.updated_at),
    });
  }
  for (const t of tasks) {
    catalog.set(t.task_id, {
      title: taskTitleFromPrompt(t.prompt),
      status: t.status ?? null,
      source: 'task',
      updatedAt: toUpdatedAtMs(t.updated_at),
    });
  }
  for (const p of canvases) {
    catalog.set(p.project_id, {
      title: (p.title ?? '').trim(),
      status: null,
      source: 'canvas',
      updatedAt: toUpdatedAtMs(p.updated_at),
    });
  }
  for (const b of briefings) {
    catalog.set(b.id, {
      title: (b.title ?? '').trim(),
      status: b.status ?? null,
      source: 'briefing',
      updatedAt: toUpdatedAtMs(b.updated_at),
    });
  }
  return catalog;
}

/**
 * Local MRU first (sessions + tasks remembered while browsing), then fill from
 * the server catalog by recency so canvas / clips / briefings are not buried
 * behind leftover agent sessions.
 */
export function mergeRecentNavItems(
  localSessions: RecentVideoGenerationEntry[],
  localTasks: RecentVideoGenerationEntry[],
  sessions: SessionLike[],
  tasks: TaskLike[],
  canvases: CanvasLike[],
  briefings: BriefingLike[],
  limit: number,
  loadedKinds?: ReadonlySet<RecentCreationKind>
): RecentNavItem[] {
  const catalog = catalogFrom(sessions, tasks, canvases, briefings);
  const out: RecentNavItem[] = [];
  const used = new Set<string>();

  const push = (id: string, cachedTitle?: string, localSource?: RecentCreationKind) => {
    if (out.length >= limit || used.has(id)) return;
    const hit = catalog.get(id);
    if (hit) {
      used.add(id);
      out.push({
        id,
        title: hit.title || cachedTitle || '',
        status: hit.status,
        source: hit.source,
      });
      return;
    }
    // Catalog miss: drop if that kind loaded (deleted). Keep if the list failed.
    if (!localSource || loadedKinds === undefined || loadedKinds.has(localSource)) return;
    used.add(id);
    out.push({
      id,
      title: cachedTitle || '',
      status: null,
      source: localSource,
    });
  };

  const local = [...localSessions, ...localTasks].sort((a, b) => b.at - a.at);
  for (const entry of local) push(entry.id, entry.title, navSource(entry.source));

  const rest = [...catalog.entries()]
    .filter(([id]) => !used.has(id))
    .sort((a, b) => b[1].updatedAt - a[1].updatedAt);
  for (const [id, hit] of rest) {
    if (out.length >= limit) break;
    used.add(id);
    out.push({
      id,
      title: hit.title,
      status: hit.status,
      source: hit.source,
    });
  }

  return out;
}

export function navItemsEqual(prev: RecentNavItem[], next: RecentNavItem[]): boolean {
  if (prev.length !== next.length) return false;
  return prev.every(
    (row, i) =>
      row.id === next[i]?.id &&
      row.title === next[i]?.title &&
      row.source === next[i]?.source &&
      (row.status ?? null) === (next[i]?.status ?? null)
  );
}

export function fallbackNavItems(
  localSessions: RecentVideoGenerationEntry[],
  localTasks: RecentVideoGenerationEntry[],
  limit: number
): RecentNavItem[] {
  const local = [...localSessions, ...localTasks].sort((a, b) => b.at - a.at);
  const out: RecentNavItem[] = [];
  const used = new Set<string>();
  for (const entry of local) {
    if (out.length >= limit || used.has(entry.id)) continue;
    used.add(entry.id);
    out.push({
      id: entry.id,
      title: entry.title?.trim() || '',
      status: null,
      source: navSource(entry.source),
    });
  }
  return out;
}
