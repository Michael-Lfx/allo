
import type { IMessagePlan, TMessage } from '@/common/chat/chatLib';

export interface PinnedPlanData {
  entries: IMessagePlan['content']['entries'];
  /** Whether the plan's owning turn is still active. */
  active: boolean;
  /** Number of entries with status === 'completed'. */
  done: number;
  /** Total number of entries. */
  total: number;
}

export type PlanDisplayStatus = 'completed' | 'in_progress' | 'incomplete';

/**
 * Header / pulse state for a plan snapshot.
 *
 * `in_progress` on a step means the agent is working on it *now*. Once the
 * owning turn is no longer live, leftover `in_progress` rows are unfinished
 * work, not a live queue — show them as incomplete instead of "In progress".
 */
export function planDisplayStatus(plan: PinnedPlanData): PlanDisplayStatus {
  if (plan.total > 0 && plan.done >= plan.total) return 'completed';
  const hasLiveStep = plan.active && plan.entries.some((entry) => entry.status === 'in_progress');
  return hasLiveStep ? 'in_progress' : 'incomplete';
}

/** Entries as the user should see them after the turn stopped. */
export function displayPlanEntries(plan: PinnedPlanData): PinnedPlanData['entries'] {
  if (plan.active) return plan.entries;
  return plan.entries.map((entry) =>
    entry.status === 'in_progress' ? { ...entry, status: 'pending' } : entry
  );
}

/**
 * Derive the *live* plan from the conversation message list.
 *
 * A plan row in history is an audit of a snapshot the model declared. The
 * workspace tab and composer chip are a current-work queue, not that archive:
 *
 * - Do not delete the row. Persistence and the model transcript keep it.
 * - Do not invent a new plan. Implementation after a finished design round
 *   may never call `update_plan` again; that is allowed.
 * - A fully completed snapshot has no remaining work, so it is not live.
 *   Hiding it is what lets a new user request start without the previous
 *   round's checklist still occupying the tab.
 * - An incomplete snapshot stays visible (even after the owning turn ends)
 *   until a newer `update_plan` replaces it.
 *
 * The current snapshot is the last `plan` message (updates reuse the same
 * `msg_id` and are moved to the tail). Empty or completed latest snapshots
 * hide the live surfaces.
 */
export function derivePinnedPlan(list: TMessage[]): PinnedPlanData | null {
  for (let i = list.length - 1; i >= 0; i--) {
    const message = list[i];
    if (message.type !== 'plan') continue;
    const entries = message.content.entries ?? [];
    if (entries.length === 0) return null;
    const done = entries.filter((entry) => entry.status === 'completed').length;
    const total = entries.length;
    if (done >= total) return null;
    return {
      entries,
      active: message.status !== 'finish' && message.status !== 'error',
      done,
      total,
    };
  }
  return null;
}
