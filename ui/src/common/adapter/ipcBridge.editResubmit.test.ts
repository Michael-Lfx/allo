import { describe, expect, test } from 'bun:test';

import { decodeEditResubmitObservation } from './ipcBridge';

const id = '0190f5fe-7c00-7a00-8000-000000000901';

const expectDecodeFailure = (value: unknown): void => {
  let failed = false;
  try {
    decodeEditResubmitObservation(value);
  } catch {
    failed = true;
  }
  expect(failed).toBe(true);
};

const accepted = () => ({
  receipt_state: 'accepted',
  delivery: {
    msg_id: id,
    replayed: true,
    completed: false,
    result_ok: null,
    result_text: null,
    result_error: null,
  },
  replacement_message_id: id,
  target_exists: true,
  replacement_exists: false,
  requires_reset: false,
});

describe('decodeEditResubmitObservation', () => {
  test('accepts an explicit-null missing receipt snapshot', () => {
    expect(
      decodeEditResubmitObservation({
        receipt_state: 'missing',
        delivery: null,
        replacement_message_id: null,
        target_exists: true,
        replacement_exists: null,
        requires_reset: false,
      })
    ).toEqual({
      receipt_state: 'missing',
      delivery: null,
      replacement_message_id: null,
      target_exists: true,
      replacement_exists: null,
      requires_reset: false,
    });
  });
  test('rejects unknown receipt states and missing booleans', () => {
    expectDecodeFailure({ ...accepted(), receipt_state: 'queued' });
    const { requires_reset: _, ...missingReset } = accepted();
    expectDecodeFailure(missingReset);
    const { target_exists: __, ...missingTarget } = accepted();
    expectDecodeFailure(missingTarget);
  });

  test('rejects delivery on missing and mismatched candidate identities', () => {
    expectDecodeFailure({ ...accepted(), receipt_state: 'missing' });
    expectDecodeFailure({
      ...accepted(),
      replacement_message_id: '0190f5fe-7c00-7a00-8000-000000000902',
    });
  });
});
