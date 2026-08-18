/** Duration for one pipeline progress event in the activity log. */

export type TimelineEvent = {
  stage: string;
  at?: string;
};

export type EventElapsed = {
  /** Whole seconds; null when timestamps cannot be resolved. */
  secs: number | null;
  /** True when this is the in-flight stage and the clock is still running. */
  live: boolean;
};

export function parseEventMs(at: string | undefined): number | null {
  if (!at) return null;
  const ms = Date.parse(at);
  return Number.isNaN(ms) ? null : ms;
}

/** Stopwatch clock: `mm:ss`, or `h:mm:ss` after an hour. */
export function formatElapsedClock(totalSecs: number): string {
  const secs = Math.max(0, Math.floor(totalSecs));
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  const mm = String(m).padStart(2, '0');
  const ss = String(s).padStart(2, '0');
  if (h > 0) return `${h}:${mm}:${ss}`;
  return `${mm}:${ss}`;
}

/**
 * Duration of `events[index]`.
 *
 * A stage starts when its event is emitted and ends when the next event
 * arrives. The last event is live while the run is still busy.
 */
export function eventElapsed(
  events: readonly TimelineEvent[],
  index: number,
  opts: { busy: boolean; nowMs: number; updatedAt?: string | null }
): EventElapsed {
  const start = parseEventMs(events[index]?.at);
  if (start == null) return { secs: null, live: false };

  const next = events[index + 1];
  if (next) {
    const end = parseEventMs(next.at);
    if (end == null) return { secs: null, live: false };
    return { secs: Math.max(0, Math.floor((end - start) / 1000)), live: false };
  }

  if (opts.busy) {
    return { secs: Math.max(0, Math.floor((opts.nowMs - start) / 1000)), live: true };
  }

  const updated = parseEventMs(opts.updatedAt ?? undefined);
  if (updated != null && updated >= start) {
    return { secs: Math.max(0, Math.floor((updated - start) / 1000)), live: false };
  }
  return { secs: null, live: false };
}
