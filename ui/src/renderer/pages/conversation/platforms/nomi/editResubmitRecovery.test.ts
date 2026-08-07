import { describe, expect, test } from 'bun:test';

import { parseMessageId } from '@/common/types/ids';
import type { IEditResubmitObservation } from '@/common/adapter/ipcBridge';
import { resolveEditResubmitRecovery } from './editResubmitRecovery';

const delivery = (overrides: Partial<NonNullable<IEditResubmitObservation['delivery']>> = {}) => ({
  msg_id: parseMessageId('0190f5fe-7c00-7a00-8000-000000000901'),
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
  replacement_message_id: delivery().msg_id,
  target_exists: true,
  replacement_exists: false,
  ...overrides,
});

describe('resolveEditResubmitRecovery', () => {
  test('keeps an accepted receipt pending while the target still exists', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation(),
        requestOutcome: 'ambiguous_failure',
      })
    ).toEqual({ kind: 'claimed_pending', shouldReconcile: false, shouldRevoke: false });
  });

  test('recognizes transcript truncation without revoking', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({ target_exists: false }),
        requestOutcome: 'ambiguous_failure',
      })
    ).toEqual({ kind: 'transcript_truncated', shouldReconcile: true, shouldRevoke: false });
  });

  test('only a terminal pre-admission failure may revoke', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({
          receipt_state: 'missing',
          delivery: null,
        }),
        requestOutcome: 'definitive_pre_admission_failure',
      })
    ).toEqual({ kind: 'safe_failure', shouldReconcile: false, shouldRevoke: true });

    expect(
      resolveEditResubmitRecovery({
        observation: observation({
          receipt_state: 'missing',
          delivery: null,
        }),
        requestOutcome: 'ambiguous_failure',
      })
    ).toEqual({ kind: 'unknown', shouldReconcile: false, shouldRevoke: false });
  });

  test('classifies an error after truncation as post-mutation failure', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({
          receipt_state: 'completed',
          delivery: delivery({ completed: true, result_ok: false }),
          target_exists: false,
        }),
        requestOutcome: 'ambiguous_failure',
      })
    ).toEqual({ kind: 'post_mutation_failure', shouldReconcile: true, shouldRevoke: false });
  });

  test('replacement identity is authoritative success evidence', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({
          target_exists: false,
          replacement_exists: true,
        }),
        requestOutcome: 'accepted',
      })
    ).toEqual({ kind: 'success', shouldReconcile: true, shouldRevoke: false });
  });

  test('completed replacement remains success when legacy result_ok is absent', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({
          receipt_state: 'completed',
          delivery: delivery({ completed: true, result_ok: null }),
          target_exists: false,
          replacement_exists: true,
        }),
        requestOutcome: 'ambiguous_failure',
      })
    ).toEqual({ kind: 'success', shouldReconcile: true, shouldRevoke: false });
  });

  test('inconsistent identities remain unknown and fail closed', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({
          target_exists: true,
          replacement_exists: true,
        }),
        requestOutcome: 'accepted',
      })
    ).toEqual({ kind: 'unknown', shouldReconcile: false, shouldRevoke: false });
  });

  test('candidate and delivery IDs must agree before reconciliation', () => {
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
    ).toEqual({ kind: 'unknown', shouldReconcile: false, shouldRevoke: false });
  });
});
