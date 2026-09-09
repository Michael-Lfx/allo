import { describe, expect, test } from 'bun:test';
import {
  classifyDownloadNetwork,
  classifyUpdateError,
  clearPendingUpdateApply,
  computeAverageBps,
  markPendingUpdateApply,
  normalizeUpdateTelemetrySource,
  resetUpdateTelemetryForTests,
  trackUpdateCheckCompleted,
  trackUpdateDownloadSucceeded,
  trackUpdateInstallFailed,
  trackUpdatePromptShown,
} from './updateTelemetry';
import { listFunnelEvents, resetFunnelForTests } from './productFunnel';
import {
  listQueuedTelemetryEventsForTests,
  resetTelemetryOutboxForTests,
} from './telemetryOutbox';
import { AUTO_INSTALL_UNSUPPORTED_ERROR } from '@/common/adapter/tauriUpdateInstall';

describe('update telemetry', () => {
  test('classifies install preflight blocks', () => {
    expect(classifyUpdateError(`${AUTO_INSTALL_UNSUPPORTED_ERROR}:mounted_volume`)).toEqual({
      error_code: 'install_blocked',
      failure_code: 'mounted_volume',
    });
  });

  test('normalizes open sources', () => {
    expect(normalizeUpdateTelemetrySource('startup')).toBe('startup');
    expect(normalizeUpdateTelemetrySource('titlebar')).toBe('titlebar');
    expect(normalizeUpdateTelemetrySource('nope')).toBe('unknown');
  });

  test('classifies slow downloads from average bps', () => {
    expect(computeAverageBps(10_000_000, 100_000)).toBe(100_000);
    expect(
      classifyDownloadNetwork({
        average_bps: 100_000,
        duration_ms: 100_000,
        bytes: 10_000_000,
      })
    ).toBe('slow');
    expect(classifyDownloadNetwork({ already_ready: true })).toBe('cache');
    expect(classifyDownloadNetwork({ failed: true })).toBe('failed');
  });

  test('queues update pipeline events as platform telemetry with allowlisted props', () => {
    resetFunnelForTests();
    resetTelemetryOutboxForTests();
    resetUpdateTelemetryForTests();

    trackUpdateCheckCompleted({
      source: 'startup',
      status: 'available',
      duration_ms: 420.6,
      from_version: 'v1.0.0',
      to_version: '1.1.0',
    });
    trackUpdatePromptShown({
      source: 'startup',
      from_version: '1.0.0',
      to_version: '1.1.0',
    });
    trackUpdateDownloadSucceeded({
      source: 'modal',
      duration_ms: 1500,
      from_version: '1.0.0',
      to_version: '1.1.0',
      bytes_total: 12_345_678,
      peak_bps: 9_000_000,
      already_ready: false,
    });
    trackUpdateInstallFailed({
      source: 'modal',
      duration_ms: 12,
      from_version: '1.0.0',
      to_version: '1.1.0',
      error: `${AUTO_INSTALL_UNSUPPORTED_ERROR}:app_translocation`,
    });

    const queued = listQueuedTelemetryEventsForTests();
    expect(queued.map((event) => event.name)).toEqual([
      'update_check_completed',
      'update_prompt_shown',
      'update_download_succeeded',
      'update_install_blocked',
    ]);
    expect(queued.every((event) => event.module === 'platform')).toBe(true);
    expect(queued[0]?.properties).toMatchObject({
      feature: 'desktop_update',
      source: 'startup',
      status: 'available',
      duration_ms: 421,
      from_version: '1.0.0',
      to_version: '1.1.0',
      cdn_host: 'modelscope.cn',
      network_class: 'fast',
    });
    expect(queued[2]?.properties.bytes_total).toBe(12_345_678);
    expect(queued[2]?.properties.average_bps).toBe(Math.round((12_345_678 * 1000) / 1500));
    expect(queued[2]?.properties.peak_bps).toBe(9_000_000);
    expect(queued[2]?.properties.network_class).toBe('fast');
    expect(queued[2]?.properties.already_ready).toBe(false);
    expect(queued[3]?.properties.failure_code).toBe('app_translocation');
    expect(queued[3]?.properties.failure_class).toBe('preflight');
    expect('prompt' in (queued[0]?.properties ?? {})).toBe(false);
  });

  test('markPendingUpdateApply is safe without browser storage', () => {
    resetUpdateTelemetryForTests();
    expect(() => markPendingUpdateApply('1.0.0', 'v1.1.0')).not.toThrow();
    expect(() => clearPendingUpdateApply()).not.toThrow();
  });

  test('records check failed with error code', () => {
    resetFunnelForTests();
    trackUpdateCheckCompleted({
      source: 'modal',
      status: 'failed',
      duration_ms: 10,
      error_code: 'network',
    });
    expect(listFunnelEvents().at(-1)?.props?.error_code).toBe('network');
    expect(listFunnelEvents().at(-1)?.props?.network_class).toBe('failed');
  });
});
