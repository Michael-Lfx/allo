import type { IEditResubmitObservation } from '@/common/adapter/ipcBridge';

export type EditResubmitRequestOutcome =
  | 'accepted'
  | 'server_rejected'
  | 'transport_ambiguous';

export type EditResubmitRecoveryKind =
  | 'safe_failure'
  | 'pending'
  | 'mutated'
  | 'requires_reset';

export interface EditResubmitRecovery {
  kind: EditResubmitRecoveryKind;
}

/**
 * Resolve one exact edit/resubmit observation. The target's durable presence
 * is the mutation boundary; replacement presence is only progress information
 * and must never be used to revoke an operation after the target disappeared.
 */
export const resolveEditResubmitRecovery = ({
  observation,
  requestOutcome,
}: {
  observation: IEditResubmitObservation;
  requestOutcome: EditResubmitRequestOutcome;
}): EditResubmitRecovery => {
  if (observation.requires_reset) {
    return { kind: 'requires_reset' };
  }

  if (
    observation.delivery !== null &&
    observation.replacement_message_id !== null &&
    observation.delivery.msg_id !== observation.replacement_message_id
  ) {
    return { kind: 'pending' };
  }

  if (observation.receipt_state === 'missing') {
    return observation.target_exists && requestOutcome === 'server_rejected'
      ? { kind: 'safe_failure' }
      : { kind: 'pending' };
  }

  // Once the exact target is gone, the destructive boundary has happened.
  // This remains true even when the replacement is not visible yet.
  if (!observation.target_exists) {
    return { kind: 'mutated' };
  }

  if (observation.receipt_state === 'completed' && observation.delivery?.result_ok === false) {
    return { kind: 'safe_failure' };
  }

  // Accepted receipts are still owned by a worker; completed receipts whose
  // target remains present are inconsistent and must be observed again.
  return { kind: 'pending' };
};
