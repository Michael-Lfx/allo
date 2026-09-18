/**
 * Conversation-row status tag (sidebar).
 *
 * A conversation row used to carry two wordless dots: a green `processing-dot`
 * and a blue `run-dot`. Neither said what was actually happening, and the run
 * dot only ever appeared for the Run the client happened to be following. This
 * module derives one readable status per row instead — the single place that
 * decides "which status wins", so the sidebar stays a pure renderer.
 *
 * Only *active* rows get a tag (a decided product call): a quiet history row
 * says nothing, and an idle row must not invent a claim like "已完成" that the
 * conversation projection cannot actually support — `ConversationStatus` is
 * only `pending | running | finished`, and `finished` covers cancelled and
 * failed chats alike.
 */

import { isTerminalRunStatus, latestRunStatus, terminalRunStatus } from "./run-notify";
import { runStatusTone, type RunStatusTone } from "./run-tree";
import type { RunEvent } from "./protocol";

export type ConversationStatusTag = {
  /** i18n key of the tag label. */
  labelKey: string;
  /** CSS class suffix: one of the five `runStatusTone` buckets. */
  tone: RunStatusTone;
};

/**
 * The tag for one conversation row, or `null` when the row is not active.
 *
 * Priority:
 *   1. the followed Run, when it is this conversation's and still moving —
 *      its own status is the most specific truth available;
 *   2. `is_processing` — a turn is running, no Run status to report;
 *   3. otherwise nothing.
 *
 * A *terminal* Run keeps no tag: the engine will not move it again, so an
 * "已完成" chip on a history row is noise, and the terminal notice already
 * surfaces the outcome once as a toast.
 */
export function conversationStatusTag(input: {
  isProcessing: boolean;
  /** Live status of the followed Run, already scoped to this row by the caller. */
  runStatus: string | null;
}): ConversationStatusTag | null {
  if (input.runStatus !== null && !isTerminalRunStatus(input.runStatus)) {
    return {
      labelKey: `run.statusValue.${input.runStatus}`,
      tone: runStatusTone(input.runStatus),
    };
  }
  if (input.isProcessing) {
    return { labelKey: "common.processing", tone: "active" };
  }
  return null;
}

/**
 * The live status of the followed Run, scoped to one conversation.
 *
 * Returns `null` unless the followed Run belongs to `conversationId` and has
 * not reached a terminal status — exactly the condition a row needs to show the
 * Run's own status instead of a generic "正在处理".
 */
export function followedRunStatus(
  conversationId: string,
  runConversationId: string | null,
  activeRunId: string | null,
  runEvents: RunEvent[],
): string | null {
  if (activeRunId === null || runConversationId !== conversationId) return null;
  if (terminalRunStatus(runEvents) !== null) return null;
  return latestRunStatus(runEvents);
}
