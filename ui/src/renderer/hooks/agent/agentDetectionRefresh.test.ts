/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import {
  createAgentRefreshScheduler,
  refreshAgentAvailability,
  shouldScheduleAgentRefreshAfterHashChange,
  shouldScheduleAgentRefreshForHash,
} from './agentDetectionRefresh';

describe('agent detection refresh scheduler', () => {
  test('runs only in the primary application renderer', () => {
    expect(shouldScheduleAgentRefreshForHash('#/guid')).toBe(true);
    expect(shouldScheduleAgentRefreshForHash('#/companion?companion_id=abc')).toBe(false);
    expect(shouldScheduleAgentRefreshForHash('#/companion/details')).toBe(false);
    expect(shouldScheduleAgentRefreshForHash('#/nomi-memory-panel')).toBe(false);
  });

  test('does not treat primary-route navigation as a refresh trigger', () => {
    expect(shouldScheduleAgentRefreshAfterHashChange('#/guid', '#/conversation/123')).toBe(false);
    expect(shouldScheduleAgentRefreshAfterHashChange('#/companion', '#/guid')).toBe(true);
  });

  test('writes the POST refreshed snapshot even without an active SWR subscriber', async () => {
    const calls: string[] = [];
    const snapshot = ['agent-a'];

    await refreshAgentAvailability({
      refreshSnapshot: async () => {
        calls.push('POST');
        return snapshot;
      },
      replaceCachedSnapshot: async (value) => {
        calls.push(`cache:${value.join(',')}`);
      },
    });

    expect(calls).toEqual(['POST', 'cache:agent-a']);
  });

  test('does not cache an empty fallback when refreshing the snapshot fails', async () => {
    const error = new Error('backend unavailable');
    let cacheWrites = 0;

    let thrown: unknown;
    try {
      await refreshAgentAvailability({
        refreshSnapshot: async () => {
          throw error;
        },
        replaceCachedSnapshot: async () => {
          cacheWrites += 1;
        },
      });
    } catch (caught) {
      thrown = caught;
    }

    expect(thrown).toBe(error);
    expect(cacheWrites).toBe(0);
  });

  test('shares an in-flight probe across callers', async () => {
    let resolveTask!: () => void;
    let calls = 0;
    const task = () => {
      calls += 1;
      return new Promise<void>((resolve) => {
        resolveTask = resolve;
      });
    };
    const refresh = createAgentRefreshScheduler({ task, now: () => 1_000 });

    const first = refresh();
    const second = refresh();
    expect(calls).toBe(1);

    resolveTask();
    await Promise.all([first, second]);
  });

  test('does not probe again until the interval expires', async () => {
    let now = 1_000;
    let calls = 0;
    const refresh = createAgentRefreshScheduler({
      task: async () => {
        calls += 1;
      },
      intervalMs: 30,
      now: () => now,
    });

    await refresh();
    now = 1_029;
    await refresh();
    expect(calls).toBe(1);

    now = 1_030;
    await refresh();
    expect(calls).toBe(2);
  });

  test('reports failures without creating an unhandled rejection', async () => {
    const error = new Error('probe failed');
    const errors: unknown[] = [];
    const refresh = createAgentRefreshScheduler({
      task: async () => {
        throw error;
      },
      onError: (value) => errors.push(value),
    });

    await refresh();
    expect(errors).toEqual([error]);
  });
});
