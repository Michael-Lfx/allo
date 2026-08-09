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

  test('rejects observation deliveries that are not durable replays', () => {
    expectDecodeFailure({
      ...accepted(),
      delivery: { ...accepted().delivery, replayed: false },
    });
  });

  test('rejects terminal metadata on accepted receipts', () => {
    expectDecodeFailure({
      ...accepted(),
      delivery: {
        ...accepted().delivery,
        result_error_code: 'provider_failed',
        result_error_retryable: true,
      },
    });
  });

  test('rejects contradictory completed success and error outcomes', () => {
    expectDecodeFailure({
      ...accepted(),
      receipt_state: 'completed',
      delivery: {
        ...accepted().delivery,
        completed: true,
        result_ok: true,
        result_text: 'replacement',
        result_error: 'cannot be both success and failure',
      },
      replacement_exists: true,
    });
  });

  test('accepts completed failures that preserve partial output', () => {
    expect(
      decodeEditResubmitObservation({
        ...accepted(),
        receipt_state: 'completed',
        delivery: {
          ...accepted().delivery,
          completed: true,
          result_ok: false,
          result_text: 'partial output',
          result_error: 'provider failed after partial output',
          result_error_code: 'provider_failed',
          result_error_retryable: false,
        },
        target_exists: false,
        replacement_exists: true,
      }).delivery
    ).toMatchObject({ result_ok: false, result_text: 'partial output' });
  });
});
