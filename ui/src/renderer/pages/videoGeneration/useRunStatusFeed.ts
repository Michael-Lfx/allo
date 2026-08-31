/**
 * Run-status feed for the ViMax workspace (`GET /api/vimax/sessions/:id/status`).
 *
 * One module-level store feeds two snapshot shapes so per-second poll updates
 * do not re-render the whole workspace:
 *
 * - `full` — complete `SessionStatus`; subscribed by the small live-progress
 *       components (StudioAgentSession, StoryboardBoard). New identity only when the payload actually changed.
 * - `flags` — coarse primitives (`status`, `busy`, `failedLike`, media paths,
 *   …) the workspace chrome derives buttons/panels from. New identity only
 *   when one of those primitives changes, so the heavy page body bails out
 *   on heartbeat ticks that merely advance stage messages.
 *
 * Scheduling is a self-perpetuating `setTimeout` chain: the next poll is only
 * scheduled after the previous one settles (no request pile-up), with a
 * hidden-tab backoff. Artifact-tree rescans stay in the page via `onTick`.
 */
import { useCallback, useEffect, useRef, useSyncExternalStore } from 'react';
import { getSessionStatus, isActiveStatus } from './api';
import type { SessionStatus, VimaxRunStatus } from './types';
import { isInsufficientCreditsError } from './creditsError';
import { resolveSessionCreditsConsumed } from './sessionCredits';

/** Coarse signals the workspace chrome subscribes to — changes rarely. */
export interface RunStatusFlags {
  status: VimaxRunStatus | null;
  busy: boolean;
  /** failed | cancelled | interrupted. */
  failedLike: boolean;
  stage: string | null;
  stagePlanned: boolean;
  hasFinalVideo: boolean;
  hasCover: boolean;
  finalVideoPath: string | null;
  coverPath: string | null;
  /** Terminal failure whose error matches the insufficient-credits shape. */
  creditsFailed: boolean;
  /** Aggregate Flowy video-task credits for this session (changes as clips bill). */
  creditsConsumed: number;
}

const EMPTY_FLAGS: RunStatusFlags = {
  status: null,
  busy: false,
  failedLike: false,
  stage: null,
  stagePlanned: false,
  hasFinalVideo: false,
  hasCover: false,
  finalVideoPath: null,
  coverPath: null,
  creditsFailed: false,
  creditsConsumed: 0,
};

const FLAG_KEYS = [
  'status',
  'busy',
  'failedLike',
  'stage',
  'stagePlanned',
  'hasFinalVideo',
  'hasCover',
  'finalVideoPath',
  'coverPath',
  'creditsFailed',
  'creditsConsumed',
] as const;

interface FeedState {
  sessionId: string;
  full: SessionStatus | null;
  flags: RunStatusFlags;
}

let state: FeedState = { sessionId: '', full: null, flags: EMPTY_FLAGS };
let lastFullJson = '';
const listeners = new Set<() => void>();

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function notify(): void {
  for (const listener of listeners) listener();
}

function computeFlags(next: SessionStatus | null, prev: RunStatusFlags): RunStatusFlags {
  const status = next?.status ?? null;
  const flags: RunStatusFlags = {
    status,
    busy: isActiveStatus(status),
    failedLike: status === 'failed' || status === 'cancelled' || status === 'interrupted',
    stage: next?.stage ?? null,
    stagePlanned: next?.stage === 'planned',
    hasFinalVideo: Boolean(next?.final_video),
    hasCover: Boolean(next?.cover),
    finalVideoPath: next?.final_video ?? null,
    coverPath: next?.cover ?? null,
    creditsFailed:
      status === 'failed' && typeof next?.error === 'string' && isInsufficientCreditsError(next.error),
    creditsConsumed: resolveSessionCreditsConsumed({
      statusCredits: next?.credits_consumed,
      events: next?.events,
    }),
  };
  for (const key of FLAG_KEYS) {
    if (!Object.is(flags[key], prev[key])) return flags;
  }
  return prev;
}

let tickHandler: ((st: SessionStatus) => void) | undefined;

function applyStatus(st: SessionStatus): void {
  const json = JSON.stringify(st);
  if (state.full === null || json !== lastFullJson) {
    state.full = st;
    lastFullJson = json;
  }
  state.flags = computeFlags(state.full, state.flags);
  notify();
  // Fired on every settled poll (not just diffs) so timing-based consumers
  // like the artifact safety net keep working.
  tickHandler?.(st);
}

/**
 * Optimistic local update after plan/render submit — same shapes the page
 * used to write through `setRunStatus`.
 */
export function patchRunStatus(patch: Partial<SessionStatus>): void {
  const base: SessionStatus =
    state.full ?? { stage: '', message: '', progress: 0, status: 'idle' };
  state.full = { ...base, ...patch };
  lastFullJson = JSON.stringify(state.full);
  state.flags = computeFlags(state.full, state.flags);
  notify();
}

function adopt(sessionId: string): void {
  if (state.sessionId === sessionId) return;
  state = { sessionId, full: null, flags: EMPTY_FLAGS };
  lastFullJson = '';
  notify();
}

let inflight: Promise<SessionStatus | null> | null = null;

/** Fetch once; concurrent callers share the request. Errors resolve to null. */
export function fetchRunStatus(): Promise<SessionStatus | null> {
  if (!state.sessionId) return Promise.resolve(null);
  if (inflight) return inflight;
  inflight = getSessionStatus(state.sessionId)
    .then((st) => {
      applyStatus(st);
      return st;
    })
    .catch((e) => {
      console.warn('[videoGeneration] status poll failed', e);
      return null;
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

/** Non-reactive read for event handlers (click callbacks, reveal-in-folder). */
export function getRunStatusSnapshot(): SessionStatus | null {
  return state.full;
}

/**
 * Mounts the polling loop for a session. Returns a manual `refresh()` used by
 * mount effects and post-submit checks.
 */
export function useRunStatusFeedController(
  sessionId: string,
  onTick?: (st: SessionStatus) => void
): () => Promise<SessionStatus | null> {
  const tickRef = useRef(onTick);
  useEffect(() => {
    tickRef.current = onTick;
  });

  useEffect(() => {
    tickHandler = tickRef.current;
    adopt(sessionId);
    let stopped = false;
    let timer: number | undefined;
    const schedule = () => {
      const active = isActiveStatus(state.full?.status);
      const ms = active && document.hidden ? 5000 : active ? 1000 : 5000;
      timer = window.setTimeout(() => {
        void cycle();
      }, ms);
    };
    const cycle = async () => {
      if (stopped || !sessionId) return;
      try {
        await fetchRunStatus();
      } finally {
        if (!stopped) schedule();
      }
    };
    void cycle();
    return () => {
      stopped = true;
      if (timer != null) window.clearTimeout(timer);
      if (tickHandler === tickRef.current) tickHandler = undefined;
    };
  }, [sessionId]);

  return useCallback(() => fetchRunStatus(), []);
}

/** Full snapshot — identity-stable across unchanged polls. Pass `false` to pause. */
export function useRunStatusFull(active = true): SessionStatus | null {
  return useSyncExternalStore(
    subscribe,
    () => (active ? state.full : null),
    () => (active ? state.full : null)
  );
}

/** Coarse primitives — the heavy page body should subscribe to this only. */
export function useRunStatusFlags(): RunStatusFlags {
  return useSyncExternalStore(
    subscribe,
    () => state.flags,
    () => state.flags
  );
}

// ── document.hidden as an external store ────────────────────────────────────

const hiddenListeners = new Set<() => void>();
if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', () => {
    for (const listener of hiddenListeners) listener();
  });
}

function subscribeHidden(listener: () => void): () => void {
  hiddenListeners.add(listener);
  return () => {
    hiddenListeners.delete(listener);
  };
}

/** Live elapsed clocks pause while the tab is hidden. */
export function useDocumentHidden(): boolean {
  return useSyncExternalStore(
    subscribeHidden,
    () => document.hidden,
    () => document.hidden
  );
}
