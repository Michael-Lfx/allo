import { describe, expect, test } from 'bun:test';

import { DomCommitRecovery } from './errorBoundaryRecovery';

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

const notFoundError = () =>
  Object.assign(
    new Error("Failed to execute 'removeChild' on 'Node': The node to be removed is not a child of this node."),
    { name: 'NotFoundError' },
  );

describe('DomCommitRecovery', () => {
  test('schedules exactly one remount per error until the rearm window passes', async () => {
    let recovered = 0;
    const recovery = new DomCommitRecovery({
      onRecover: () => {
        recovered += 1;
      },
      recoveryDelayMs: 0,
      rearmMs: 40,
    });

    expect(recovery.maybeSchedule(notFoundError())).toBe(true);
    // A second crash while the first remount is pending stays on the panel.
    expect(recovery.maybeSchedule(notFoundError())).toBe(false);
    await sleep(15);
    expect(recovered).toBe(1);

    await sleep(50);
    expect(recovery.maybeSchedule(notFoundError())).toBe(true);
    await sleep(15);
    expect(recovered).toBe(2);
    recovery.dispose();
  });

  test('ignores errors that are not detached-node commit failures', async () => {
    let recovered = 0;
    const recovery = new DomCommitRecovery({
      onRecover: () => {
        recovered += 1;
      },
      recoveryDelayMs: 0,
    });

    expect(recovery.maybeSchedule(new Error('route component crashed'))).toBe(false);
    await sleep(15);
    expect(recovered).toBe(0);
    recovery.dispose();
  });

  test('reset cancels a pending remount and clears the latch', async () => {
    let recovered = 0;
    const recovery = new DomCommitRecovery({
      onRecover: () => {
        recovered += 1;
      },
      recoveryDelayMs: 20,
    });

    expect(recovery.maybeSchedule(notFoundError())).toBe(true);
    recovery.reset();
    await sleep(40);
    expect(recovered).toBe(0);
    expect(recovery.maybeSchedule(notFoundError())).toBe(true);
    await sleep(40);
    expect(recovered).toBe(1);
    recovery.dispose();
  });

  test('dispose cancels pending remounts when the boundary unmounts', async () => {
    let recovered = 0;
    const recovery = new DomCommitRecovery({
      onRecover: () => {
        recovered += 1;
      },
      recoveryDelayMs: 20,
    });

    expect(recovery.maybeSchedule(notFoundError())).toBe(true);
    recovery.dispose();
    await sleep(40);
    expect(recovered).toBe(0);
  });
});
