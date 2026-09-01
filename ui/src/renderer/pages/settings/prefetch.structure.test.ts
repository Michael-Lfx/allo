/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const prefetchSource = readFileSync(new URL('./prefetch.ts', import.meta.url), 'utf8');
const footerSource = readFileSync(
  new URL('../../components/layout/Sider/SiderFooter.tsx', import.meta.url),
  'utf8'
);

const settingsSiderSource = readFileSync(
  new URL('./components/SettingsSider.tsx', import.meta.url),
  'utf8'
);

describe('settings route prefetch', () => {
  test('warms the settings page, sider, and default system panel chunks', () => {
    expect(prefetchSource.includes("void import('./SystemSettings')")).toBe(true);
    expect(prefetchSource.includes("void import('./components/SettingsSider')")).toBe(true);
    expect(
      prefetchSource.includes(
        "void import('@/renderer/components/settings/SettingsModal/contents/SystemModalContent')"
      )
    ).toBe(true);
    expect(prefetchSource.includes("from './SystemSettings'")).toBe(false);
  });

  test('warms intelligence-group settings panels before the sider click', () => {
    expect(prefetchSource.includes("void import('./PoiSettings')")).toBe(true);
    expect(prefetchSource.includes("void import('./LearningSettings')")).toBe(true);
    expect(prefetchSource.includes("void import('./InsightsSettings')")).toBe(true);
    expect(prefetchSource.includes("void import('./MoaSettings')")).toBe(true);
    expect(prefetchSource.includes("void import('./MediaSettings')")).toBe(true);
    expect(prefetchSource.includes("from './LearningSettings'")).toBe(false);
  });

  test('warms capability hubs before their first installed/market switch', () => {
    expect(prefetchSource.includes("void import('./PresetSettings')")).toBe(true);
    expect(prefetchSource.includes("void import('./SkillsSettingsPage')")).toBe(true);
    expect(prefetchSource.includes("void import('@/renderer/pages/mcp')")).toBe(true);
    expect(prefetchSource.includes("void import('@/renderer/pages/mcp/PluginSettingsPage')")).toBe(true);
  });

  test('the sider settings button prefetches on hover and idle', () => {
    expect(
      footerSource.includes("import { prefetchSettingsPages } from '@renderer/pages/settings/prefetch'")
    ).toBe(true);
    expect(footerSource.includes('onPointerEnter={() => prefetchSettingsPages()}')).toBe(true);
    expect(footerSource.includes('prefetchSettingsPages()')).toBe(true);
  });

  test('settings sider remount also warms route chunks (idle skip while in settings)', () => {
    expect(settingsSiderSource.includes("import { prefetchSettingsPages } from '../prefetch'")).toBe(
      true
    );
    expect(settingsSiderSource.includes('prefetchSettingsPages()')).toBe(true);
  });
});
