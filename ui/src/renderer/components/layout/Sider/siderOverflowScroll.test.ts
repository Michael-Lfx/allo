/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');

describe('application sider overflow handling', () => {
  test('keeps primary navigation fixed and scrolls only the workspaces list', () => {
    expect(source.includes("data-testid='sider-primary-nav'")).toBe(true);
    expect(source.includes("className='shrink-0 flex flex-col gap-2px'")).toBe(true);
    expect(source.includes("data-testid='sider-workspaces-scroll-area'")).toBe(true);
    expect(source.includes('flex-1 min-h-0 overflow-y-auto overflow-x-hidden pl-5px')).toBe(true);
  });

  test('keeps the settings group pinned', () => {
    expect(source.includes("'shrink-0 mt-auto pt-8px flex flex-col gap-2px")).toBe(true);
  });
});
