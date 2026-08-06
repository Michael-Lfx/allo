/**
 * Remember Video Generation workspace sessions so the sider can restore the
 * last project and show a short MRU list under the nav entry.
 */

const LAST_SESSION_KEY = 'flowy.videoGeneration.lastSessionId';
/** Survives app restarts — used for the sider "recent 3" strip. */
const RECENT_SESSIONS_KEY = 'flowy.videoGeneration.recentSessions';

/** How many recent projects the sider shows. */
export const RECENT_VIDEO_GENERATION_VISIBLE = 3;
/** Cap stored MRU entries (localStorage). */
const RECENT_VIDEO_GENERATION_STORE_LIMIT = 12;

export interface RecentVideoGenerationEntry {
  id: string;
  /** Cached title for instant sider paint before listSessions resolves. */
  title?: string;
  /** Epoch ms when last opened / remembered. */
  at: number;
}

function readStorage(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStorage(key: string, value: string): void {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    /* private mode / quota — ignore */
  }
}

function removeStorage(key: string): void {
  try {
    window.localStorage.removeItem(key);
  } catch {
    /* ignore */
  }
}

function readSessionStorage(key: string): string | null {
  try {
    return window.sessionStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeSessionStorage(key: string, value: string): void {
  try {
    window.sessionStorage.setItem(key, value);
  } catch {
    /* ignore */
  }
}

function removeSessionStorage(key: string): void {
  try {
    window.sessionStorage.removeItem(key);
  } catch {
    /* ignore */
  }
}

function normalizeTitle(title: string | null | undefined): string | undefined {
  const t = (title ?? '').trim();
  return t || undefined;
}

export function readRecentVideoGenerationSessions(): RecentVideoGenerationEntry[] {
  try {
    const raw = readStorage(RECENT_SESSIONS_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    const out: RecentVideoGenerationEntry[] = [];
    const seen = new Set<string>();
    for (const item of parsed) {
      if (!item || typeof item !== 'object') continue;
      const id = typeof (item as { id?: unknown }).id === 'string'
        ? (item as { id: string }).id.trim()
        : '';
      if (!id || seen.has(id)) continue;
      seen.add(id);
      const title = normalizeTitle((item as { title?: string }).title);
      const at =
        typeof (item as { at?: unknown }).at === 'number' &&
        Number.isFinite((item as { at: number }).at)
          ? (item as { at: number }).at
          : Date.now();
      out.push(title ? { id, title, at } : { id, at });
    }
    return out;
  } catch {
    return [];
  }
}

function writeRecentVideoGenerationSessions(entries: RecentVideoGenerationEntry[]): void {
  writeStorage(RECENT_SESSIONS_KEY, JSON.stringify(entries.slice(0, RECENT_VIDEO_GENERATION_STORE_LIMIT)));
}

/**
 * Remember the last opened session (sider entry restore) and update the MRU list.
 *
 * Order of the visible top-N stays stable when re-opening a session already in
 * that window — only brand-new (or out-of-window) sessions insert at the front.
 * @param title optional display title cached for the sider strip
 */
export function rememberVideoGenerationSession(
  sessionId: string | null | undefined,
  title?: string | null
): void {
  const id = (sessionId ?? '').trim();
  if (!id) return;
  writeSessionStorage(LAST_SESSION_KEY, id);

  const cachedTitle = normalizeTitle(title);
  const now = Date.now();
  const prevAll = readRecentVideoGenerationSessions();
  const existingIdx = prevAll.findIndex((e) => e.id === id);

  // Already inside the visible strip — update title / timestamp in place, keep order.
  if (existingIdx >= 0 && existingIdx < RECENT_VIDEO_GENERATION_VISIBLE) {
    const next = prevAll.map((entry, idx) => {
      if (idx !== existingIdx) return entry;
      return {
        ...entry,
        at: now,
        title: cachedTitle ?? entry.title,
      };
    });
    writeRecentVideoGenerationSessions(next);
    return;
  }

  // New project (or was beyond the visible window) — insert at front.
  const rest = prevAll.filter((e) => e.id !== id);
  const previous = existingIdx >= 0 ? prevAll[existingIdx] : undefined;
  const next: RecentVideoGenerationEntry = { id, at: now };
  if (cachedTitle) {
    next.title = cachedTitle;
  } else if (previous?.title) {
    next.title = previous.title;
  }
  writeRecentVideoGenerationSessions([next, ...rest]);
}

export function clearVideoGenerationSessionMemory(sessionId?: string | null): void {
  const id = (sessionId ?? '').trim();
  if (id) {
    const current = readSessionStorage(LAST_SESSION_KEY);
    if (!current || current === id) {
      removeSessionStorage(LAST_SESSION_KEY);
    }
    writeRecentVideoGenerationSessions(
      readRecentVideoGenerationSessions().filter((e) => e.id !== id)
    );
    return;
  }
  removeSessionStorage(LAST_SESSION_KEY);
  removeStorage(RECENT_SESSIONS_KEY);
}

/** Update a cached title without changing MRU order (sider refresh). */
export function updateRecentVideoGenerationTitle(
  sessionId: string | null | undefined,
  title: string | null | undefined
): void {
  const id = (sessionId ?? '').trim();
  const nextTitle = normalizeTitle(title);
  if (!id || !nextTitle) return;
  const list = readRecentVideoGenerationSessions();
  let changed = false;
  const next = list.map((entry) => {
    if (entry.id !== id || entry.title === nextTitle) return entry;
    changed = true;
    return { ...entry, title: nextTitle };
  });
  if (changed) writeRecentVideoGenerationSessions(next);
}

export function readRememberedVideoGenerationSession(): string | null {
  const id = readSessionStorage(LAST_SESSION_KEY)?.trim() ?? '';
  return id || null;
}

/** Path for sider / deep-link restore: last workspace, else list home. */
export function videoGenerationEntryPath(): string {
  const id = readRememberedVideoGenerationSession();
  return id ? `/video-generation/${id}` : '/video-generation';
}

/**
 * Merge local MRU with server session list: prefer browse order, drop deleted,
 * fill gaps from server `updated_at` order, return at most `limit` items.
 */
export function mergeRecentVideoGenerationProjects(
  recent: RecentVideoGenerationEntry[],
  sessions: Array<{ id: string; title?: string | null }>,
  limit = RECENT_VIDEO_GENERATION_VISIBLE
): Array<{ id: string; title: string }> {
  const byId = new Map(
    sessions.map((s) => [s.id, (s.title ?? '').trim() || ''])
  );
  const out: Array<{ id: string; title: string }> = [];
  const used = new Set<string>();

  for (const entry of recent) {
    if (out.length >= limit) break;
    if (!byId.has(entry.id)) continue; // deleted / unknown
    used.add(entry.id);
    const title = byId.get(entry.id) || entry.title || '';
    out.push({ id: entry.id, title });
  }

  for (const s of sessions) {
    if (out.length >= limit) break;
    if (used.has(s.id)) continue;
    used.add(s.id);
    out.push({ id: s.id, title: (s.title ?? '').trim() });
  }

  return out;
}
