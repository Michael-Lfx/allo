/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const workpathDrawerSource = readFileSync(new URL('./WorkpathDrawer.tsx', import.meta.url), 'utf8');
const companionGroupSource = readFileSync(new URL('./CompanionSessionGroup.tsx', import.meta.url), 'utf8');
const overflowButtonSource = readFileSync(new URL('./SessionOverflowButton.tsx', import.meta.url), 'utf8');

describe('SessionList overflow controls', () => {
  test('shares semantic overflow controls across mounted session groups', () => {
    for (const source of [workpathDrawerSource, companionGroupSource]) {
      expect(source.includes("import SessionOverflowButton from './SessionOverflowButton';")).toBe(true);
      expect(source.includes('<SessionOverflowButton')).toBe(true);
    }
    expect(workpathDrawerSource.includes("className='flowy-workpath-session-overflow'")).toBe(true);
    expect(companionGroupSource.includes("className='flowy-companion-session-overflow'")).toBe(true);
    expect(overflowButtonSource.includes("t('sessionList.expandDisplay'")).toBe(true);
    expect(overflowButtonSource.includes('aria-expanded={expanded}')).toBe(true);
    expect(overflowButtonSource.includes('aria-controls={controlsId}')).toBe(true);
    expect(overflowButtonSource.includes("type='button'")).toBe(true);
    expect(overflowButtonSource.includes('session-overflow-button')).toBe(true);
  });
});
