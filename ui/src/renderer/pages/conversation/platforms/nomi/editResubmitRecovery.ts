import type { IEditResubmitObservation } from '@/common/adapter/ipcBridge';

export type EditResubmitRequestOutcome =
  | 'accepted'
  | 'ambiguous_failure'
  | 'definitive_pre_admission_failure';

export type EditResubmitRecoveryKind =
  | 'safe_failure'
  | 'claimed_pending'
  | 'transcript_truncated'
  | 'success'
  | 'post_mutation_failure'
  | 'unknown';

export interface EditResubmitRecovery {
  kind: EditResubmitRecoveryKind;
  shouldReconcile: boolean;
  shouldRevoke: boolean;
}

/**
 * Resolve an edit/resubmit observation without treating a missing window row as
 * proof of anything. The caller must keep polling/replaying for `unknown` and
 * `claimed_pending`; neither outcome may revoke the durable barrier or mint a
 * replacement operation key.
 */
export const resolveEditResubmitRecovery = ({
  observation,
  requestOutcome,
}: {
  observation: IEditResubmitObservation;
  requestOutcome: EditResubmitRequestOutcome;
}): EditResubmitRecovery => {
  const { receipt_state: receiptState, delivery, target_exists: targetExists, replacement_exists: replacementExists } =
    observation;

  if (
    delivery !== null &&
    observation.replacement_message_id !== null &&
    delivery.msg_id !== observation.replacement_message_id
  ) {
    return { kind: 'unknown', shouldReconcile: false, shouldRevoke: false };
  }

  if (targetExists && replacementExists === true) {
    if (receiptState === 'completed' && delivery?.result_ok === false) {
      return { kind: 'post_mutation_failure', shouldReconcile: true, shouldRevoke: false };
    }
    return { kind: 'unknown', shouldReconcile: false, shouldRevoke: false };
  }

  if (replacementExists === true) {
    if (receiptState === 'completed' && delivery?.result_ok === false) {
      return { kind: 'post_mutation_failure', shouldReconcile: true, shouldRevoke: false };
    }
    if (
      receiptState === 'accepted' ||
      (receiptState === 'completed' && delivery?.result_ok !== false)
    ) {
      return { kind: 'success', shouldReconcile: true, shouldRevoke: false };
    }
    return { kind: 'unknown', shouldReconcile: false, shouldRevoke: false };
  }

  if (!targetExists && replacementExists === false) {
    if (receiptState === 'accepted' && delivery !== null) {
      return { kind: 'transcript_truncated', shouldReconcile: true, shouldRevoke: false };
    }
    if (receiptState === 'completed' && delivery?.result_ok === false) {
      return { kind: 'post_mutation_failure', shouldReconcile: true, shouldRevoke: false };
    }
    return { kind: 'unknown', shouldReconcile: false, shouldRevoke: false };
  }

  if (targetExists && replacementExists === false) {
    if (receiptState === 'completed' && delivery?.result_ok === false) {
      return { kind: 'safe_failure', shouldReconcile: false, shouldRevoke: true };
    }
    if (
      receiptState === 'missing' &&
      requestOutcome === 'definitive_pre_admission_failure'
    ) {
      return { kind: 'safe_failure', shouldReconcile: false, shouldRevoke: true };
    }
    if (receiptState === 'accepted' && delivery !== null) {
      return { kind: 'claimed_pending', shouldReconcile: false, shouldRevoke: false };
    }
  }

  return { kind: 'unknown', shouldReconcile: false, shouldRevoke: false };
};
