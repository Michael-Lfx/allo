import { describe, expect, test } from 'bun:test';

import { parseMessageId } from '@/common/types/ids';
import type { IEditResubmitObservation } from '@/common/adapter/ipcBridge';
import { resolveEditResubmitRecovery, shouldReplayEditResubmit } from './editResubmitRecovery';

const replacementId = parseMessageId('0190f5fe-7c00-7a00-8000-000000000901');

const delivery = (overrides: Partial<NonNullable<IEditResubmitObservation['delivery']>> = {}) => ({
  msg_id: replacementId,
  replayed: true,
  completed: false,
  result_ok: null,
  result_text: null,
  result_error: null,
  ...overrides,
});

const observation = (
  overrides: Partial<IEditResubmitObservation> = {}
): IEditResubmitObservation => ({
  receipt_state: 'accepted',
  delivery: delivery(),
  replacement_message_id: replacementId,
  target_exists: true,
  replacement_exists: false,
  requires_reset: false,
  ...overrides,
});

describe('resolveEditResubmitRecovery', () => {
  test('server rejection plus missing receipt and present target is safe to revoke', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({
          receipt_state: 'missing',
          delivery: null,
          replacement_message_id: null,
          replacement_exists: null,
        }),
        requestOutcome: 'server_rejected',
      })
    ).toEqual({ kind: 'safe_failure' });
  });

  test('ambiguous transport failure plus missing receipt remains unknown', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({
          receipt_state: 'missing',
          delivery: null,
          replacement_message_id: null,
          replacement_exists: null,
        }),
        requestOutcome: 'transport_ambiguous',
      })
    ).toEqual({ kind: 'unknown' });
  });

  test('accepted receipt with target present remains pending', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({ target_exists: true, replacement_exists: false }),
        requestOutcome: 'accepted',
      })
    ).toEqual({ kind: 'claimed_pending' });
  });

  test('accepted target absence proves transcript truncation', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({ target_exists: false, replacement_exists: false }),
        requestOutcome: 'accepted',
      })
    ).toEqual({ kind: 'transcript_truncated' });

    expect(
      resolveEditResubmitRecovery({
        observation: observation({ target_exists: false, replacement_exists: true }),
        requestOutcome: 'accepted',
      })
    ).toEqual({ kind: 'transcript_truncated' });
  });

  test('completed error with target present requires reset and never revokes', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({
          receipt_state: 'completed',
          delivery: delivery({ completed: true, result_ok: false }),
          target_exists: true,
        }),
        requestOutcome: 'server_rejected',
      })
    ).toEqual({ kind: 'requires_reset' });
  });

  test('completed error after target removal remains a mutation', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({
          receipt_state: 'completed',
          delivery: delivery({ completed: true, result_ok: false }),
          target_exists: false,
          replacement_exists: false,
        }),
        requestOutcome: 'transport_ambiguous',
      })
    ).toEqual({ kind: 'post_mutation_failure' });
  });

  test('authoritative reset requirement stops every recovery path', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({
          requires_reset: true,
          receipt_state: 'missing',
          delivery: null,
          replacement_message_id: null,
          target_exists: true,
          replacement_exists: null,
        }),
        requestOutcome: 'server_rejected',
      })
    ).toEqual({ kind: 'requires_reset' });
  });

  test('candidate and delivery IDs must agree before mutation handling', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({
          target_exists: false,
          replacement_exists: true,
          replacement_message_id: parseMessageId(
            '0190f5fe-7c00-7a00-8000-000000000902'
          ),
        }),
        requestOutcome: 'accepted',
      })
    ).toEqual({ kind: 'unknown' });
  });

  test('missing receipt after external target removal is post-mutation failure', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({
          receipt_state: 'missing',
          delivery: null,
          replacement_message_id: null,
          target_exists: false,
          replacement_exists: null,
        }),
        requestOutcome: 'transport_ambiguous',
      })
    ).toEqual({ kind: 'post_mutation_failure', notice: 'target_changed' });
  });

  test('completed success requires the exact replacement to exist', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({
          receipt_state: 'completed',
          delivery: delivery({ completed: true, result_ok: true }),
          target_exists: false,
          replacement_exists: true,
        }),
        requestOutcome: 'accepted',
      })
    ).toEqual({ kind: 'success' });

    expect(
      resolveEditResubmitRecovery({
        observation: observation({
          receipt_state: 'completed',
          delivery: delivery({ completed: true, result_ok: true }),
          target_exists: false,
          replacement_exists: false,
        }),
        requestOutcome: 'accepted',
      })
    ).toEqual({ kind: 'post_mutation_failure' });
  });

  test('accepted receipts are GET-only and ambiguous missing receipts replay once per cycle', () => {
    const acceptedObservation = observation();
    expect(
      shouldReplayEditResubmit({
        recovery: { kind: 'claimed_pending' },
        observation: acceptedObservation,
        requestOutcome: 'accepted',
        replayedThisCycle: false,
      })
    ).toBe(false);
    const missingObservation = observation({
      receipt_state: 'missing',
      delivery: null,
      replacement_message_id: null,
      replacement_exists: null,
    });
    expect(
      shouldReplayEditResubmit({
        recovery: { kind: 'unknown' },
        observation: missingObservation,
        requestOutcome: 'transport_ambiguous',
        replayedThisCycle: false,
      })
    ).toBe(true);
    expect(
      shouldReplayEditResubmit({
        recovery: { kind: 'unknown' },
        observation: missingObservation,
        requestOutcome: 'transport_ambiguous',
        replayedThisCycle: true,
      })
    ).toBe(false);
  });
});
