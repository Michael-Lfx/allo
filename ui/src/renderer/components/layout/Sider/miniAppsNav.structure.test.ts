/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

const siderSource = readSource(new URL('./index.tsx', import.meta.url));
const entrySource = readSource(new URL('./SiderNav/SiderMiniAppsEntry.tsx', import.meta.url));
const routerSource = readSource(new URL('../Router.tsx', import.meta.url));

describe('mini-apps rail navigation', () => {
  test('the home rail hides the mini-app module', () => {
    expect(siderSource.includes('SiderMiniAppsEntry')).toBe(false);
    expect(siderSource.includes("navTo('/mini-apps')")).toBe(false);
    expect(siderSource.includes("pathname.startsWith('/mini-apps')")).toBe(false);
  });

  test('the entry labels itself from the miniApps namespace with a plain icon import', () => {
    expect(entrySource.includes("t('miniApps.nav.entry')")).toBe(true);
    expect(entrySource.includes("import { ApplicationOne } from '@icon-park/react';")).toBe(true);
    // An aliased icon import survives tsc but the build-time icon rewrite turns
    // it into illegal syntax, so the module 500s at runtime.
    const iconImportLine = entrySource.split('\n').find((line) => line.includes('@icon-park/react')) ?? '';
    expect(iconImportLine.includes(' as ')).toBe(false);
    expect(iconImportLine.includes('* ')).toBe(false);
  });

  test('both mini-app routes are registered behind the route fallback', () => {
    expect(routerSource.includes("path='/mini-apps' element={withRouteFallback(MiniAppsListPage)}")).toBe(true);
    expect(routerSource.includes("path='/mini-apps/:id' element={withRouteFallback(MiniAppRunnerPage)}")).toBe(true);
    expect(routerSource.includes("import('@renderer/pages/miniApps')")).toBe(true);
    expect(routerSource.includes("import('@renderer/pages/miniApps/RunnerPage')")).toBe(true);
  });
});
