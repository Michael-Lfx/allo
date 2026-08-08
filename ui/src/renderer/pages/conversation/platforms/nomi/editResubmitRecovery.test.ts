import { describe, expect, test } from 'bun:test';

import { parseMessageId } from '@/common/types/ids';
import type { IEditResubmitObservation } from '@/common/adapter/ipcBridge';
import { resolveEditResubmitRecovery } from './editResubmitRecovery';

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
        }),
        requestOutcome: 'server_rejected',
      })
    ).toEqual({ kind: 'safe_failure' });
  });

  test('ambiguous transport failure plus missing receipt remains pending', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({
          receipt_state: 'missing',
          delivery: null,
          replacement_message_id: null,
        }),
        requestOutcome: 'transport_ambiguous',
      })
    ).toEqual({ kind: 'pending' });
  });

  test('accepted receipt with target present remains pending', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({ target_exists: true, replacement_exists: false }),
        requestOutcome: 'accepted',
      })
    ).toEqual({ kind: 'pending' });
  });

  test('target absence is mutation proof even before replacement appears', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({ target_exists: false, replacement_exists: false }),
        requestOutcome: 'accepted',
      })
    ).toEqual({ kind: 'mutated' });

    expect(
      resolveEditResubmitRecovery({
        observation: observation({ target_exists: false, replacement_exists: true }),
        requestOutcome: 'accepted',
      })
    ).toEqual({ kind: 'mutated' });
  });

  test('completed error with target present is a safe failure', () => {
    expect(
      resolveEditResubmitRecovery({
        observation: observation({
          receipt_state: 'completed',
          delivery: delivery({ completed: true, result_ok: false }),
          target_exists: true,
        }),
        requestOutcome: 'server_rejected',
      })
    ).toEqual({ kind: 'safe_failure' });
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
    ).toEqual({ kind: 'mutated' });
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
    ).toEqual({ kind: 'pending' });
  });
});
