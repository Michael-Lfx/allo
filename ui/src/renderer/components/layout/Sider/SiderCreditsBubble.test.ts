/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const bubbleSource = readFileSync(new URL('./SiderCreditsBubble.tsx', import.meta.url), 'utf8');
const userMenuSource = readFileSync(new URL('./SiderUserMenu.tsx', import.meta.url), 'utf8');

describe('SiderCreditsBubble structure and integration', () => {
  test('imports and mounts SiderCreditsBubble inside SiderUserMenu with proper positioning', () => {
    expect(userMenuSource.includes('import SiderCreditsBubble from')).toBe(true);
    expect(userMenuSource.includes('<SiderCreditsBubble collapsed={collapsed} isMobile={isMobile} />')).toBe(true);
  });

  test('handles low credits and exhausted credits states with differentiated copy and theme classes', () => {
    expect(bubbleStateKey(bubbleSource, 'common.creditsBubble.exhaustedTitle')).toBe(true);
    expect(bubbleStateKey(bubbleSource, 'common.creditsBubble.lowTitle')).toBe(true);
    expect(bubbleStateKey(bubbleSource, 'common.creditsBubble.exhaustedDesc')).toBe(true);
    expect(bubbleStateKey(bubbleSource, 'common.creditsBubble.lowDesc')).toBe(true);
    expect(bubbleSource.includes('var(--danger)')).toBe(true);
    expect(bubbleSource.includes('var(--flowy-attention)')).toBe(true);
    expect(bubbleSource.includes('sider-credits-bubble--expanded')).toBe(true);
    expect(bubbleSource.includes('sider-credits-bubble--collapsed')).toBe(true);
  });

  test('triggers openOfficialWebsiteCredits on top up action', () => {
    expect(bubbleSource.includes('openOfficialWebsiteCredits')).toBe(true);
    expect(bubbleSource.includes("source: 'sider'")).toBe(true);
  });

  test('integrates anti-fatigue dismiss logic', () => {
    expect(bubbleSource.includes('dismissCreditsBubble')).toBe(true);
    expect(bubbleSource.includes('isCreditsBubbleDismissed')).toBe(true);
  });
});

function bubbleStateKey(source: string, key: string): boolean {
  return source.includes(key);
}
