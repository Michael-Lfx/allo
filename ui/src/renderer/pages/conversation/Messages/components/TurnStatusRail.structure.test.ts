/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./TurnStatusRail.tsx', import.meta.url), 'utf8');

describe('TurnStatusRail', () => {
  test('hides busy writing status above the composer and keeps permission wait', () => {
    expect(source.includes("case 'waiting_permission':")).toBe(true);
    expect(source.includes("case 'streaming':")).toBe(true);
    expect(source.includes('shouldShowLiveRail')).toBe(true);
    expect(source.includes('data-testid=\'turn-status-rail\'')).toBe(true);
  });
});
