import { isBackendHttpError } from '@/common/adapter/httpBridge';
import type { I18nKey } from '@/renderer/services/i18n/i18n-keys';

export type ConversationDeleteFailureKind =
  | 'attemptRetained'
  | 'runningOrphan'
  | 'pending'
  | 'unknown';

export type ConversationDeleteBatchFailure = {
  kind: ConversationDeleteFailureKind;
  reason: unknown;
};

export type ConversationDeleteBatchSummary = {
  successCount: number;
  failureCounts: Map<ConversationDeleteFailureKind, number>;
  failures: ConversationDeleteBatchFailure[];
};

export function classifyConversationDeleteError(error: unknown): ConversationDeleteFailureKind {
  if (!isBackendHttpError(error)) return 'unknown';

  switch (error.code) {
    case 'CONVERSATION_ATTEMPT_RETAINED':
      return 'attemptRetained';
    case 'CONVERSATION_RUNNING_ORPHAN':
      return 'runningOrphan';
    case 'CONVERSATION_DELETE_PENDING':
      return 'pending';
    default:
      return 'unknown';
  }
}

export function conversationDeleteMessageKey(
  kind: ConversationDeleteFailureKind,
  batch: boolean,
): I18nKey {
  if (batch) {
    switch (kind) {
      case 'attemptRetained':
        return 'conversation.history.batchDeleteAttemptRetained';
      case 'runningOrphan':
        return 'conversation.history.batchDeleteRunningOrphan';
      case 'pending':
        return 'conversation.history.batchDeletePending';
      default:
        return 'conversation.history.batchDeleteFailed';
    }
  }

  switch (kind) {
    case 'attemptRetained':
      return 'conversation.history.deleteAttemptRetained';
    case 'runningOrphan':
      return 'conversation.history.deleteRunningOrphan';
    case 'pending':
      return 'conversation.history.deletePending';
    default:
      return 'conversation.history.deleteFailed';
  }
}

export function summarizeConversationDeleteResults(
  results: readonly PromiseSettledResult<boolean>[],
): ConversationDeleteBatchSummary {
  let successCount = 0;
  const failureCounts = new Map<ConversationDeleteFailureKind, number>();
  const failures: ConversationDeleteBatchFailure[] = [];

  for (const result of results) {
    if (result.status === 'fulfilled' && result.value) {
      successCount += 1;
      continue;
    }

    const reason = result.status === 'rejected' ? result.reason : undefined;
    const kind = classifyConversationDeleteError(reason);
    failureCounts.set(kind, (failureCounts.get(kind) ?? 0) + 1);
    failures.push({ kind, reason });
  }

  return { successCount, failureCounts, failures };
}
