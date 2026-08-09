import { describe, expect, test } from 'bun:test';

import { runSingleFlight } from './resetSingleFlight';

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
});
