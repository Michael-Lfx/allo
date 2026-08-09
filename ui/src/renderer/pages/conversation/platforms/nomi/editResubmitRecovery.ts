import type { IEditResubmitObservation } from '@/common/adapter/ipcBridge';

export type EditResubmitRequestOutcome =
  | 'accepted'
  | 'server_rejected'
  | 'transport_ambiguous';

export type EditResubmitRecoveryKind =
  | 'safe_failure'
  | 'claimed_pending'
  | 'transcript_truncated'
  | 'success'
  | 'post_mutation_failure'
  | 'unknown'
  | 'requires_reset';

export interface EditResubmitRecovery {
  kind: EditResubmitRecoveryKind;
}

export const shouldReplayEditResubmit = ({
  recovery,
  observation,
  requestOutcome,
  replayedThisCycle,
}: {
  recovery: EditResubmitRecovery;
  observation: IEditResubmitObservation;
  requestOutcome: EditResubmitRequestOutcome;
  replayedThisCycle: boolean;
}): boolean =>
  recovery.kind === 'unknown' &&
  observation.receipt_state === 'missing' &&
  requestOutcome === 'transport_ambiguous' &&
  !replayedThisCycle;

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
    return { kind: 'unknown' };
  }

  if (observation.receipt_state === 'missing') {
    if (!observation.target_exists) {
      return { kind: 'post_mutation_failure' };
    }
    return requestOutcome === 'server_rejected'
      ? { kind: 'safe_failure' }
      : { kind: 'unknown' };
  }

  if (observation.receipt_state === 'accepted') {
    return observation.target_exists
      ? { kind: 'claimed_pending' }
      : { kind: 'transcript_truncated' };
  }

  if (observation.target_exists) {
    return { kind: 'requires_reset' };
  }

  return observation.delivery?.result_ok === true && observation.replacement_exists === true
    ? { kind: 'success' }
    : { kind: 'post_mutation_failure' };
};
