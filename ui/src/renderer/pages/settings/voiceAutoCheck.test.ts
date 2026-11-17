import { describe, expect, test } from 'bun:test';
import { autoCheckVoiceCapabilities, shouldAutoCheckVoiceCapabilities } from './voiceAutoCheck';
import type { VoiceCapabilityReport } from '@common/adapter/voiceCompanion';

const report = (ready: boolean, checkedAt = 1): VoiceCapabilityReport =>
  ({ can_start_full_call: ready, checked_at_ms: checkedAt }) as unknown as VoiceCapabilityReport;

describe('automatic readiness check after a download', () => {
  test('stops at the first ready report without sleeping', async () => {
    const sleeps: number[] = [];
    const result = await autoCheckVoiceCapabilities({
      runSelfCheck: async () => report(true),
      sleep: async (ms) => {
        sleeps.push(ms);
      },
      attempts: 3,
      retryDelayMs: 2000,
    });
    expect(result.ready).toBe(true);
    expect(result.attemptsUsed).toBe(1);
    expect(sleeps).toEqual([]);
  });

  test('retries while the daemon is still warming up and succeeds on a later round', async () => {
    let calls = 0;
    const sleeps: number[] = [];
    const result = await autoCheckVoiceCapabilities({
      runSelfCheck: async () => {
        calls += 1;
        return report(calls >= 3, calls);
      },
      sleep: async (ms) => {
        sleeps.push(ms);
      },
      attempts: 3,
      retryDelayMs: 2000,
    });
    expect(result.ready).toBe(true);
    expect(result.attemptsUsed).toBe(3);
    expect(calls).toBe(3);
    expect(sleeps).toEqual([2000, 2000]);
  });

  test('stays in checking until every attempt is spent, then returns the last report', async () => {
    let calls = 0;
    const result = await autoCheckVoiceCapabilities({
      runSelfCheck: async () => {
        calls += 1;
        return report(false, calls);
      },
      sleep: async () => {},
      attempts: 3,
    });
    expect(result.ready).toBe(false);
    expect(result.attemptsUsed).toBe(3);
    expect(calls).toBe(3);
    expect(result.report?.checked_at_ms).toBe(3);
    expect(result.budgetExhausted).toBe(false);
  });

  test('the total deadline stops a wedged daemon instead of holding the loading state forever', async () => {
    let calls = 0;
    let clock = 0;
    const result = await autoCheckVoiceCapabilities({
      runSelfCheck: async () => {
        calls += 1;
        return report(false, calls);
      },
      sleep: async (ms) => {
        clock += ms;
      },
      now: () => clock,
      attempts: 5,
      retryDelayMs: 2000,
      totalBudgetMs: 3000,
    });
    expect(result.budgetExhausted).toBe(true);
    expect(result.attemptsUsed).toBe(2);
    expect(calls).toBe(2);
  });

  test('a concurrent background check is awaited within the remaining budget', async () => {
    let calls = 0;
    let remainingSeen = -1;
    const result = await autoCheckVoiceCapabilities({
      runSelfCheck: async () => {
        calls += 1;
        throw Object.assign(new Error('self_check_in_progress'), { code: 'self_check_in_progress' });
      },
      waitForFreshReport: async (_previous, remainingMs) => {
        remainingSeen = remainingMs;
        return report(true, 42);
      },
      sleep: async () => {},
      attempts: 3,
      totalBudgetMs: 15_000,
    });
    expect(result.ready).toBe(true);
    expect(result.attemptsUsed).toBe(1);
    expect(calls).toBe(1);
    expect(remainingSeen).toBeGreaterThan(0);
    expect(result.report?.checked_at_ms).toBe(42);
  });

  test('cancellation stops further attempts', async () => {
    let calls = 0;
    let cancelled = false;
    const result = await autoCheckVoiceCapabilities({
      runSelfCheck: async () => {
        calls += 1;
        return report(false, calls);
      },
      sleep: async () => {
        cancelled = true;
      },
      isCancelled: () => cancelled,
      attempts: 5,
    });
    expect(calls).toBe(1);
    expect(result.cancelled).toBe(true);
    expect(result.attemptsUsed).toBe(1);
  });
});

describe('automatic readiness check trigger', () => {
  test('runs right after the pack became ready, before any report was read', () => {
    expect(
      shouldAutoCheckVoiceCapabilities({
        modelStatus: 'Ready',
        voiceEnabled: true,
        becameReady: true,
        capabilitiesLoaded: false,
        hasReadyReport: false,
      }),
    ).toBe(true);
  });

  test('a page visit that already has a ready report stays read-only', () => {
    expect(
      shouldAutoCheckVoiceCapabilities({
        modelStatus: 'Ready',
        voiceEnabled: true,
        becameReady: false,
        capabilitiesLoaded: true,
        hasReadyReport: true,
      }),
    ).toBe(false);
  });

  test('runs when the stored report is missing or not ready', () => {
    const base = {
      modelStatus: 'Ready',
      voiceEnabled: true,
      becameReady: false,
      capabilitiesLoaded: true,
    };
    expect(shouldAutoCheckVoiceCapabilities({ ...base, hasReadyReport: false })).toBe(true);
  });

  test('waits for the first report read instead of firing on mount', () => {
    expect(
      shouldAutoCheckVoiceCapabilities({
        modelStatus: 'Ready',
        voiceEnabled: true,
        becameReady: false,
        capabilitiesLoaded: false,
        hasReadyReport: false,
      }),
    ).toBe(false);
  });

  test('never runs while the pack is not ready or the service is disabled', () => {
    expect(
      shouldAutoCheckVoiceCapabilities({
        modelStatus: 'Downloading',
        voiceEnabled: true,
        becameReady: false,
        capabilitiesLoaded: true,
        hasReadyReport: false,
      }),
    ).toBe(false);
    expect(
      shouldAutoCheckVoiceCapabilities({
        modelStatus: 'Ready',
        voiceEnabled: false,
        becameReady: true,
        capabilitiesLoaded: true,
        hasReadyReport: false,
      }),
    ).toBe(false);
  });
});
