import { isBackendHttpError } from '@/common/adapter/httpBridge';

export type ConversationDeleteFailureKind =
  | 'attemptRetained'
  | 'runningOrphan'
  | 'pending'
  | 'unknown';

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
): string {
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
