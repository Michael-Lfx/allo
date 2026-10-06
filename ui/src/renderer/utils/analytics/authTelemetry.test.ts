/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import * as bunTest from 'bun:test';
import {
  listQueuedTelemetryEventsForTests,
  resetTelemetryOutboxForTests,
} from './telemetryOutbox';
import { resetFunnelForTests } from './productFunnel';

const { beforeEach, describe, expect, mock, test } = bunTest as typeof bunTest & {
  mock: { module: (specifier: string, factory: () => unknown) => void };
};

mock.module('@/common/adapter/tauriUpdater', () => ({
  tauriUpdateCurrentVersion: async () => '1.5.8',
}));

const {
  trackOtpSendFailed,
  trackOtpSendStarted,
  trackOtpSendSucceeded,
} = await import('./authTelemetry');

beforeEach(() => {
  resetFunnelForTests();
  resetTelemetryOutboxForTests();
});

describe('auth OTP send telemetry', () => {
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
