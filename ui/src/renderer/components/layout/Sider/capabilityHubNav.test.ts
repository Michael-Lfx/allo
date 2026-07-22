/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('capability hub navigation', () => {
  test('uses compact Remote and Open labels for the Open Capabilities tab', () => {
    const zhSettings = JSON.parse(
      readSource(new URL('../../../services/i18n/locales/zh-CN/settings.json', import.meta.url))
    );
    const enSettings = JSON.parse(
      readSource(new URL('../../../services/i18n/locales/en-US/settings.json', import.meta.url))
    );

    expect(zhSettings.openCapabilities.title).toBe('远程&开放能力');
    expect(zhSettings.openCapabilities.railTitle).toBe('远程&开放能力');
    expect(enSettings.openCapabilities.title).toBe('Remote & Open');
    expect(enSettings.openCapabilities.railTitle).toBe('Remote & Open');
  });

  test('collapses presets, skills, and MCP under Config and promotes Open Capabilities', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));

    expect(siderSource.includes('SiderConfigGroup')).toBe(true);
    expect(siderSource.includes('SiderOpenCapabilitiesEntry')).toBe(true);
    expect(siderSource.includes("navTo('/open-capabilities')")).toBe(true);
    expect(siderSource.includes('siderSection.config')).toBe(true);
    expect(siderSource.includes('<SiderPresetEntry')).toBe(false);
    expect(siderSource.includes('<SiderSkillsEntry')).toBe(false);
    expect(siderSource.includes('<SiderMcpEntry')).toBe(false);
    expect(siderSource.includes('SiderExtensionsEntry')).toBe(false);
  });

  test('keeps the first-task rail focused and progressively reveals capabilities', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));

    expect(siderSource.includes('flowy.sider.capabilitiesExpanded')).toBe(true);
    expect(siderSource.includes('common.siderSection.moreCapabilities')).toBe(true);
    expect(siderSource.includes('capabilitiesExpanded || !isSessionRoute')).toBe(true);
  });

  test('keeps presets/skills/mcp routes and hosts Open Capabilities as a first-class page', () => {
    const routerSource = readSource(new URL('../Router.tsx', import.meta.url));
    const settingsSiderSource = readSource(
      new URL('../../../pages/settings/components/SettingsSider.tsx', import.meta.url)
    );

    expect(routerSource.includes("path='/open-capabilities'")).toBe(true);
    expect(routerSource.includes("path='/settings/open-capabilities'")).toBe(true);
    expect(settingsSiderSource.includes("'open-capabilities'")).toBe(true);
    expect(routerSource.includes("path='/settings/webui' element={<Navigate to='/open-capabilities'")).toBe(true);
    expect(routerSource.includes("path='/settings/tools' element={<Navigate to='/open-capabilities'")).toBe(true);
    expect(routerSource.includes('getHashRouteRedirectUrl')).toBe(true);
    expect(routerSource.includes("path='/mcp'")).toBe(true);
    expect(routerSource.includes("path='/presets'")).toBe(true);
    expect(routerSource.includes("path='/skills'")).toBe(true);
    expect(routerSource.includes('LegacyExtensionsRedirect')).toBe(true);
    expect(routerSource.includes("path='/extensions'")).toBe(true);
  });
});
