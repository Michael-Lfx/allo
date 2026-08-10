/**
 * 编辑重发回调的纯决策逻辑（从 SendBox 的 .then/.catch 抽出以便单测）。
 *
 * 行为契约（与 C2 时序一致）：
 * - 陈旧操作（isCurrentOperation=false）：任何回调都不触碰 UI 状态（含 isLoading）。
 * - 成功 + 当前操作 + 输入未变：清空输入并退出编辑态。
 * - 成功 + 当前操作 + 输入已变（飞行中改输入）：保留当前输入，仅退出本次编辑事务。
 * - 失败 + 当前操作 + 输入未变：恢复已提交文本，保持编辑态以便重试。
 * - 失败 + 当前操作 + 输入已变：保留当前输入与编辑态，旧失败回调不得覆盖。
 * - 当前操作结束时（.finally） lowering isLoading。
 *
 * Pure decision logic for the edit-resubmit callback chain (extracted from
 * SendBox's .then/.catch so it can be unit-tested without a React render).
 *
 * Behavioral contract (mirrors the C2 timing fix):
 * - Stale operation (isCurrentOperation=false): no callback touches UI state,
 *   including isLoading.
 * - Success + current + revision unchanged: clear the input and exit edit mode.
 * - Success + current + revision changed (user typed mid-flight): keep the
 *   current input, only retire this edit transaction.
 * - Failure + current + revision unchanged: restore the submitted text and stay
 *   in edit mode so the user can retry.
 * - Failure + current + revision changed: keep the current input and edit mode;
 *   the stale failure callback must not overwrite it.
 * - clearLoading is set for the current operation's .finally regardless of status.
 */
export type EditResubmitStatus = 'success' | 'safe_failure' | 'post_mutation_failure';

export interface EditResubmitOutcomeInput {
  /** Whether this callback belongs to the still-active operation token. */
  isCurrentOperation: boolean;
  /** True when the composer input has not changed since submit (revision equal). */
  revisionUnchanged: boolean;
  status: EditResubmitStatus;
}

export interface EditResubmitOutcome {
  /** Stale operation: perform no UI side effects at all. */
  stale: boolean;
  /** Reset the composer input to '' (success path, revision unchanged). */
  clearInput: boolean;
  /** Exit edit mode: setEditingMsgId(null) + clear the prev-draft ref (success path). */
  exitEditMode: boolean;
  /** Restore the submitted text into the composer (failure path, revision unchanged). */
  restoreSubmittedInput: boolean;
  /** Lower isLoading in .finally (current operation only; status-agnostic). */
  clearLoading: boolean;
}

export const resolveEditResubmitOutcome = ({
  isCurrentOperation,
  revisionUnchanged,
  status,
}: EditResubmitOutcomeInput): EditResubmitOutcome => {
  if (!isCurrentOperation) {
    return {
      stale: true,
      clearInput: false,
      exitEditMode: false,
      restoreSubmittedInput: false,
      clearLoading: false,
    };
  }
  if (status === 'success') {
    return {
      stale: false,
      clearInput: revisionUnchanged,
      exitEditMode: true,
      restoreSubmittedInput: false,
      clearLoading: true,
    };
  }
  if (status === 'post_mutation_failure') {
    return {
      stale: false,
      clearInput: false,
      exitEditMode: true,
      restoreSubmittedInput: false,
      clearLoading: true,
    };
  }
  return {
    stale: false,
    clearInput: false,
    exitEditMode: false,
    restoreSubmittedInput: revisionUnchanged,
    clearLoading: true,
  };
};
