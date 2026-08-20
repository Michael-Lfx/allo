/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const prefetchSource = readFileSync(new URL('./prefetch.ts', import.meta.url), 'utf8');
const entrySource = readFileSync(
  new URL('../../components/layout/Sider/SiderNav/SiderScheduledEntry.tsx', import.meta.url),
  'utf8'
);

describe('scheduled tasks route prefetch', () => {
  test('warms only the list page chunk and stays free of dialog imports', () => {
    expect(prefetchSource.includes("void import('./ScheduledTasksPage')")).toBe(true);
    expect(prefetchSource.includes('CreateTaskDialog')).toBe(false);
    expect(prefetchSource.includes('GuidModelSelector')).toBe(false);
  });

  test('the sider entry prefetches on hover and idle', () => {
    expect(entrySource.includes("import { prefetchScheduledTasksPage } from '@renderer/pages/cron/prefetch'")).toBe(
      true
    );
    expect(entrySource.includes('onPointerEnter={() => prefetchScheduledTasksPage()}')).toBe(true);
    expect(entrySource.includes('requestIdleCallback')).toBe(true);
    expect(entrySource.includes('prefetchScheduledTasksPage()')).toBe(true);
  });
});
