/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const layoutSource = readFileSync(new URL('./Layout.tsx', import.meta.url), 'utf8');
const modalSource = readFileSync(new URL('../settings/UpdateModal.tsx', import.meta.url), 'utf8');
const layoutCss = readFileSync(new URL('../../styles/layout.css', import.meta.url), 'utf8');

describe('global update availability entry', () => {
  test('shows the running app version below the Flowy wordmark', () => {
    expect(layoutSource.includes("const healthGet = httpGet<{ version?: string }>('/health');")).toBe(true);
    expect(layoutSource.includes("className='sidebar-app-version'>v{appVersion}</span>")).toBe(true);
    expect(layoutCss.includes('.sidebar-app-version {')).toBe(true);
    expect(layoutCss.includes('font-size: var(--flowy-text-micro, 11px)')).toBe(true);
  });

  test('does not render the sidebar Logo update button', () => {
    expect(layoutSource.includes("className='sidebar-update-button'")).toBe(false);
    expect(layoutSource.includes("detail: { source: 'sidebar' }")).toBe(false);
  });

  test('keeps startup and modal checks connected to the shared state', () => {
    expect(layoutSource.includes('reportUpdateAvailable(res.data.updateInfo.version)')).toBe(true);
    expect(layoutSource.includes('reportNoUpdateAvailable()')).toBe(true);
    expect(modalSource.includes('reportUpdateAvailable(res.data.latest.version)')).toBe(true);
    expect(modalSource.includes('reportUpdateAvailable(evt.version)')).toBe(true);
  });
});
