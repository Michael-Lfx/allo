/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { collectSupportLogUserInfo } from './collectSupportLogUserInfo';

describe('collectSupportLogUserInfo', () => {
  test('maps cloud whoami fields and stamps collectedAt', () => {
    const info = collectSupportLogUserInfo({
      authenticated: true,
      userId: 'u-1',
      username: 'alice',
      email: 'alice@example.com',
      plan: 'Pro Plan',
      planCode: 'ProPlan',
      serverBaseUrl: 'https://cloud.example',
    });

    expect(info.userId).toBe('u-1');
    expect(info.username).toBe('alice');
    expect(info.email).toBe('alice@example.com');
    expect(info.plan).toBe('Pro Plan');
    expect(info.planCode).toBe('ProPlan');
    expect(info.serverBaseUrl).toBe('https://cloud.example');
    expect(Number.isNaN(Date.parse(info.collectedAt))).toBe(false);
  });

  test('returns empty identity fields when whoami is missing', () => {
    const info = collectSupportLogUserInfo(null);
    expect(info.userId).toBeUndefined();
    expect(info.username).toBeUndefined();
    expect(info.email).toBeUndefined();
    expect(Number.isNaN(Date.parse(info.collectedAt))).toBe(false);
  });
});
