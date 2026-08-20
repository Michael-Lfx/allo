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

export type CoalescedTimelineEvent<T extends TimelineEvent = TimelineEvent> = {
  /** First event in a consecutive same-stage run. */
  event: T;
  /** Index of that first event in the original chronological list. */
  index: number;
  /** How many consecutive events were collapsed into this row. */
  count: number;
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
 * Collapse consecutive events that share the same stage so the activity log
 * does not repeat identical labels (e.g. many `plan_scene` fan-out rows).
 */
export function coalesceProgressEvents<T extends TimelineEvent>(
  events: readonly T[]
): CoalescedTimelineEvent<T>[] {
  const out: CoalescedTimelineEvent<T>[] = [];
  for (let index = 0; index < events.length; index += 1) {
    const event = events[index];
    if (!event) continue;
    const last = out[out.length - 1];
    if (last && last.event.stage === event.stage) {
      last.count += 1;
      continue;
    }
    out.push({ event, index, count: 1 });
  }
  return out;
}

/**
 * Duration of `events[index]`.
 *
 * A stage starts when its event is emitted and ends when the next event
 * arrives (or `untilIndex` when callers have coalesced a same-stage run).
 * The last event is live while the run is still busy.
 */
export function eventElapsed(
  events: readonly TimelineEvent[],
  index: number,
  opts: { busy: boolean; nowMs: number; updatedAt?: string | null; untilIndex?: number }
): EventElapsed {
  const start = parseEventMs(events[index]?.at);
  if (start == null) return { secs: null, live: false };

  const nextIndex = opts.untilIndex ?? index + 1;
  const next = events[nextIndex];
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
