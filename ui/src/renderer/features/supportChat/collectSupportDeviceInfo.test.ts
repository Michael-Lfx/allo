/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { collectSupportDeviceInfo } from './collectSupportDeviceInfo';

describe('collectSupportDeviceInfo', () => {
  test('maps provided device status fields and stamps collectedAt', async () => {
    const info = await collectSupportDeviceInfo({
      authenticated: true,
      serialNumber: 'SN-1',
      appVersion: '1.2.3',
      activatedForVersion: true,
      lastReportedIp: '1.2.3.4',
    });
    expect(info.serialNumber).toBe('SN-1');
    expect(info.appVersion).toBe('1.2.3');
    expect(info.activatedForVersion).toBe(true);
    expect(info.lastReportedIp).toBe('1.2.3.4');
    expect(Number.isNaN(Date.parse(info.collectedAt))).toBe(false);
  });
});
