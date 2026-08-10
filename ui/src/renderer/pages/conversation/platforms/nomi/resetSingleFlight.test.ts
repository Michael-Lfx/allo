import { describe, expect, test } from 'bun:test';

import { parseConversationId } from '@/common/types/ids';

import {
  runConversationResetSingleFlight,
  runSingleFlight,
} from './resetSingleFlight';

describe('runSingleFlight', () => {
  test('a deferred same-tick double click invokes reset IPC once', async () => {
    let resolve!: () => void;
    let calls = 0;
    const deferred = new Promise<void>((done) => {
      resolve = done;
    });
    const ref = { current: null as Promise<void> | null };
    const reset = () =>
      runSingleFlight(ref, async () => {
        calls += 1;
        await deferred;
      });

    const first = reset();
    const second = reset();
    expect(first).toBe(second);
    expect(calls).toBe(1);
    resolve();
    await first;
    expect(ref.current).toBeNull();
  });

  test('the production reset handler invokes IPC and lifecycle callbacks once', async () => {
    const conversationId = parseConversationId('0190f5fe-7c00-7a00-8000-000000000921');
    let resolve!: () => void;
    const deferred = new Promise<void>((done) => {
      resolve = done;
    });
    const ref = { current: null as Promise<void> | null };
    const invocations: string[] = [];
    let starts = 0;
    let successes = 0;
    let settled = 0;
    const reset = () =>
      runConversationResetSingleFlight({
        inFlightRef: ref,
        conversationId,
        invokeReset: async ({ conversation_id }) => {
          invocations.push(conversation_id);
          await deferred;
        },
        onStart: () => {
          starts += 1;
        },
        onSuccess: () => {
          successes += 1;
        },
        onError: () => {
          throw new Error('unexpected reset failure');
        },
        onSettled: () => {
          settled += 1;
        },
      });

    const first = reset();
    const second = reset();
    expect(first).toBe(second);
    await Promise.resolve();
    expect(invocations).toEqual([conversationId]);
    expect(starts).toBe(1);
    resolve();
    await first;
    expect(successes).toBe(1);
    expect(settled).toBe(1);
  });
});
