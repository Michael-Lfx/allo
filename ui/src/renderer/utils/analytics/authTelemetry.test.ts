/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import * as bunTest from 'bun:test';
import * as updater from '@/common/adapter/tauriUpdater';
import {
  listQueuedTelemetryEventsForTests,
  resetTelemetryOutboxForTests,
} from './telemetryOutbox';
import { resetFunnelForTests } from './productFunnel';

const { beforeEach, afterEach, describe, expect, spyOn, test } = bunTest as typeof bunTest & {
  spyOn: (module: typeof updater, method: 'tauriUpdateCurrentVersion') => {
    mockResolvedValue(value: string): void;
    mockRestore(): void;
  };
};
let versionSpy: ReturnType<typeof spyOn>;

const {
  trackOtpSendFailed,
  trackOtpSendStarted,
  trackOtpSendSucceeded,
} = await import('./authTelemetry');

beforeEach(() => {
  versionSpy = spyOn(updater, 'tauriUpdateCurrentVersion');
  versionSpy.mockResolvedValue('1.5.8');
  resetFunnelForTests();
  resetTelemetryOutboxForTests();
});

afterEach(() => versionSpy.mockRestore());

describe('auth OTP send telemetry', () => {
  test('preserves the updater exports needed by other auth entry points', () => {
    expect(typeof updater.tauriUpdatePackageSnapshot).toBe('function');
    expect(typeof updater.tauriUpdateCheck).toBe('function');
  });
  test('emits started / succeeded / failed with app_version for dashboards', async () => {
    await trackOtpSendStarted();
    await trackOtpSendSucceeded();
    await trackOtpSendFailed('transport');

    const queued = listQueuedTelemetryEventsForTests();
    expect(queued.map((event) => [event.name, event.module])).toEqual([
      ['otp_send_started', 'platform'],
      ['otp_send_succeeded', 'platform'],
      ['otp_send_failed', 'platform'],
    ]);
    expect(queued.every((event) => event.properties.app_version === '1.5.8')).toBe(true);
    expect(queued[2]?.properties.failure_kind).toBe('transport');
  });
});
