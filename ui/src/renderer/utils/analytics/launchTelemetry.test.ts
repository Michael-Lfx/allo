import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import {
  flushLaunchTelemetryForTests,
  markLaunchAuthReady,
  markLaunchBootStarted,
  markLaunchConfigReady,
  markLaunchFailed,
  markLaunchInteractive,
  pendingLaunchTelemetryCountForTests,
  resetLaunchTelemetryForTests,
} from './launchTelemetry';
import { listFunnelEvents, resetFunnelForTests, trackFunnelEvent } from './productFunnel';
import {
  listQueuedTelemetryEventsForTests,
  resetTelemetryOutboxForTests,
} from './telemetryOutbox';

function seedLargeFunnelHistory(count: number): void {
  for (let i = 0; i < count; i += 1) {
    trackFunnelEvent('app_opened', { source: `seed-${i}` });
  }
}

describe('launch telemetry', () => {
  test('defers persistence: marks do not write funnel/outbox until flush', () => {
    resetFunnelForTests();
    resetTelemetryOutboxForTests();
    resetLaunchTelemetryForTests();
    seedLargeFunnelHistory(200);

    const launchBefore = listFunnelEvents().filter((event) =>
      event.name.startsWith('app_launch_')
    ).length;
    const outboxBefore = listQueuedTelemetryEventsForTests().length;

    markLaunchBootStarted();
    markLaunchAuthReady({ status: 'authenticated' });
    markLaunchConfigReady();
    markLaunchInteractive({ source: 'shell' });

    expect(pendingLaunchTelemetryCountForTests()).toBe(4);
    expect(listQueuedTelemetryEventsForTests()).toHaveLength(outboxBefore);
    expect(
      listFunnelEvents().filter((event) => event.name.startsWith('app_launch_')).length
    ).toBe(launchBefore);

    flushLaunchTelemetryForTests();

    const names = listQueuedTelemetryEventsForTests()
      .filter((event) => event.name.startsWith('app_launch_'))
      .map((event) => event.name);
    expect(names).toEqual([
      'app_launch_auth_ready',
      'app_launch_config_ready',
      'app_launch_interactive',
      'app_launch_completed',
    ]);
    expect(pendingLaunchTelemetryCountForTests()).toBe(0);
    for (const event of listQueuedTelemetryEventsForTests().filter((e) =>
      e.name.startsWith('app_launch_')
    )) {
      expect(event.module).toBe('platform');
      expect(event.properties.feature).toBe('app_launch');
      expect(event.properties.cold_start).toBe(true);
      expect(typeof event.properties.duration_ms).toBe('number');
    }
  });

  test('sync mark path stays cheap even with a large funnel history', () => {
    resetFunnelForTests();
    resetTelemetryOutboxForTests();
    resetLaunchTelemetryForTests();
    seedLargeFunnelHistory(500);

    const started = performance.now();
    markLaunchBootStarted();
    markLaunchAuthReady({ status: 'authenticated' });
    markLaunchConfigReady();
    markLaunchInteractive({ source: 'shell' });
    const syncMs = performance.now() - started;

    // Hot path must not JSON-parse/stringify the funnel/outbox queues.
    expect(syncMs).toBeLessThan(5);
    expect(pendingLaunchTelemetryCountForTests()).toBe(4);
    expect(
      listQueuedTelemetryEventsForTests().some((event) => event.name.startsWith('app_launch_'))
    ).toBe(false);
  });

  test('queues the full cold-start pipeline as platform events with timings', () => {
    resetFunnelForTests();
    resetTelemetryOutboxForTests();
    resetLaunchTelemetryForTests();

    markLaunchBootStarted();
    markLaunchAuthReady({ status: 'authenticated' });
    markLaunchConfigReady();
    markLaunchInteractive({ source: 'shell' });
    flushLaunchTelemetryForTests();

    const names = listQueuedTelemetryEventsForTests().map((event) => event.name);
    expect(names).toEqual([
      'app_launch_auth_ready',
      'app_launch_config_ready',
      'app_launch_interactive',
      'app_launch_completed',
    ]);
    const completed = listQueuedTelemetryEventsForTests().find(
      (event) => event.name === 'app_launch_completed'
    );
    expect(completed?.properties.outcome).toBe('succeeded');
    expect(completed?.properties.total_ms).toBeTypeOf('number');
  });

  test('marks login as interactive without waiting for config', () => {
    resetFunnelForTests();
    resetTelemetryOutboxForTests();
    resetLaunchTelemetryForTests();

    markLaunchBootStarted();
    markLaunchAuthReady({ status: 'unauthenticated' });
    markLaunchInteractive({ source: 'login' });
    flushLaunchTelemetryForTests();

    const names = listQueuedTelemetryEventsForTests().map((event) => event.name);
    expect(names).toEqual([
      'app_launch_auth_ready',
      'app_launch_interactive',
      'app_launch_completed',
    ]);
    expect(listFunnelEvents().some((event) => event.name === 'app_launch_config_ready')).toBe(
      false
    );
  });

  test('launch failure is terminal and skips completed', () => {
    resetFunnelForTests();
    resetTelemetryOutboxForTests();
    resetLaunchTelemetryForTests();

    markLaunchBootStarted();
    markLaunchAuthReady({ status: 'authenticated' });
    markLaunchFailed({
      error_code: 'startup_config_failed',
      blocker: 'TypeError',
      phase: 'config',
    });
    markLaunchConfigReady();
    markLaunchInteractive({ source: 'shell' });
    flushLaunchTelemetryForTests();

    const queued = listQueuedTelemetryEventsForTests();
    expect(queued.map((event) => event.name)).toEqual([
      'app_launch_auth_ready',
      'app_launch_failed',
    ]);
    expect(queued[1]?.properties.error_code).toBe('startup_config_failed');
    expect(queued[1]?.properties.blocker).toBe('TypeError');
    expect(queued[1]?.properties.outcome).toBe('failed');
  });

  test('phase marks are idempotent within one boot', () => {
    resetFunnelForTests();
    resetTelemetryOutboxForTests();
    resetLaunchTelemetryForTests();

    markLaunchBootStarted();
    expect(markLaunchAuthReady({ status: 'authenticated' })).toBe(true);
    expect(markLaunchAuthReady({ status: 'authenticated' })).toBe(false);
    expect(markLaunchConfigReady()).toBe(true);
    expect(markLaunchConfigReady()).toBe(false);
    expect(markLaunchInteractive({ source: 'shell' })).toBe(true);
    expect(markLaunchInteractive({ source: 'guid' })).toBe(false);
    flushLaunchTelemetryForTests();
    expect(
      listQueuedTelemetryEventsForTests().filter((event) => event.name === 'app_launch_interactive')
    ).toHaveLength(1);
  });

  test('uses scheduleDeferred so production flush is off the critical path', () => {
    const source = readFileSync(new URL('./launchTelemetry.ts', import.meta.url), 'utf8');
    expect(source.includes("import { scheduleDeferred }")).toBe(true);
    expect(source.includes('scheduleLaunchTelemetryFlush')).toBe(true);
    expect(source.includes('drainPendingLaunchEvents')).toBe(true);
  });
});
