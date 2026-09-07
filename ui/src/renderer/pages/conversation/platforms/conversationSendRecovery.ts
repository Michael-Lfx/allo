import {
  isBackendHttpError,
  isBackendRequestError,
} from '@/common/adapter/httpBridge';

export type ConversationSendFailureKind =
  | 'turn_admission_conflict'
  | 'ambiguous_transport'
  | 'terminal';

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

/**
 * The lifecycle admission conflict is intentionally identified by structured
 * backend metadata. Its display text is diagnostic only and must not become a
 * client protocol.
 */
export const isConversationTurnAdmissionConflict = (error: unknown): boolean => {
  if (!isBackendHttpError(error) || error.status !== 409 || !isRecord(error.details)) {
    return false;
  }

  return (
    error.details.kind === 'conversation_turn_admission' &&
    error.details.retryable === true &&
    error.details.reconcile === 'conversation.get'
  );
};

/**
 * A request that produced no response, or a server error, may have crossed the
 * durable admission boundary. Retrying the same idempotency key is safe; using
 * a new key is not.
 */
export const isAmbiguousConversationDeliveryError = (error: unknown): boolean => {
  if (isBackendRequestError(error)) {
    return error.kind === 'timeout' || error.kind === 'network';
  }

  return isBackendHttpError(error) && error.status >= 500 && error.status <= 599;
};

export const classifyConversationSendFailure = (error: unknown): ConversationSendFailureKind => {
  if (isConversationTurnAdmissionConflict(error)) return 'turn_admission_conflict';
  if (isAmbiguousConversationDeliveryError(error)) return 'ambiguous_transport';
  return 'terminal';
};
