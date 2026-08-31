import type { SessionStatus } from './types';

type CreditEvent = NonNullable<SessionStatus['events']>[number];

function maxPositive(...values: Array<number | null | undefined>): number {
  let out = 0;
  for (const value of values) {
    const n = Number(value ?? 0) || 0;
    if (n > out) out = n;
  }
  return out;
}

/**
 * Terminal `video_credits` events are one-per-clip (unlike collapsed `video_poll`).
 * Sum the latest bill per task id so the header matches what Flowy charged.
 */
export function creditsFromSessionEvents(events: CreditEvent[] | null | undefined): number {
  const byTask = new Map<number, number>();
  for (const event of events ?? []) {
    if (event.stage !== 'video_credits') continue;
    const meta = event.metadata as { task_id?: unknown; credits_consumed?: unknown } | null;
    const taskId = Number(meta?.task_id);
    const credits = Number(meta?.credits_consumed);
    if (!Number.isFinite(taskId) || taskId <= 0) continue;
    if (!Number.isFinite(credits) || credits <= 0) continue;
    byTask.set(taskId, Math.max(byTask.get(taskId) ?? 0, credits));
  }
  let sum = 0;
  for (const credits of byTask.values()) sum += credits;
  return sum;
}

export function resolveSessionCreditsConsumed(input: {
  sessionCredits?: number | null;
  statusCredits?: number | null;
  events?: CreditEvent[] | null;
}): number {
  return maxPositive(
    input.sessionCredits,
    input.statusCredits,
    creditsFromSessionEvents(input.events)
  );
}
