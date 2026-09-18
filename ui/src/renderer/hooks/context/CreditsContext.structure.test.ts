/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./CreditsContext.tsx', import.meta.url), 'utf8');

describe('credits balance refresh', () => {
  test('refreshes the sider balance after turn consumption', () => {
    expect(source.includes("emitter.on('nomi.credits.balance.refresh'")).toBe(true);
    expect(source.includes("emitter.on('nomi.turn_credits.updated'")).toBe(true);
    expect(source.includes('pendingBalanceRefreshRef')).toBe(true);
  });
});
