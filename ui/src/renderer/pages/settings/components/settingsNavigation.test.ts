

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('settings navigation', () => {
  test('hides desktop companion, learning, and requirements from settings navigation', () => {
    const siderSource = readSource(new URL('./SettingsSider.tsx', import.meta.url));
    const pageWrapperSource = readSource(new URL('./SettingsPageWrapper.tsx', import.meta.url));

    for (const id of ['nomi', 'learn', 'requirements']) {
      expect(siderSource.includes(`id: '${id}'`)).toBe(false);
      expect(pageWrapperSource.includes(`id: '${id}'`)).toBe(false);
    }
  });

  test('hides execution engines and open capabilities from settings navigation', () => {
    const siderSource = readSource(new URL('./SettingsSider.tsx', import.meta.url));
    const pageWrapperSource = readSource(new URL('./SettingsPageWrapper.tsx', import.meta.url));

    for (const id of [
      'system',
      'browser-use',
      'computer-use',
      'poi',
      'insights',
      'media',
      'presets',
      'skills',
      'mcp',
      'cloud-login',
      'about',
    ]) {
      expect(siderSource.includes(`'${id}'`)).toBe(true);
      expect(pageWrapperSource.includes(`id: '${id}'`)).toBe(true);
    }

    expect(siderSource.includes("'open-capabilities'")).toBe(false);
    expect(pageWrapperSource.includes("id: 'open-capabilities'")).toBe(false);

    expect(siderSource.includes("  'execution-engines',")).toBe(false);
    expect(siderSource.includes("id: 'execution-engines'")).toBe(false);
    expect(pageWrapperSource.includes("id: 'execution-engines'")).toBe(false);
    expect(siderSource.indexOf("'system'")).toBeLessThan(siderSource.indexOf("'browser-use'"));
    expect(siderSource.indexOf("'browser-use'")).toBeLessThan(siderSource.indexOf("'computer-use'"));
    expect(siderSource.indexOf("'computer-use'")).toBeLessThan(siderSource.indexOf("'cloud-login'"));
    expect(siderSource.indexOf("'cloud-login'")).toBeLessThan(siderSource.indexOf("'about'"));
    expect(siderSource.indexOf("'presets'")).toBeLessThan(siderSource.indexOf("'skills'"));
    expect(siderSource.indexOf("'skills'")).toBeLessThan(siderSource.indexOf("'mcp'"));
    expect(siderSource.indexOf("'mcp'")).toBeLessThan(siderSource.indexOf("'cloud-login'"));
  });

  test('routes execution engines directly and keeps legacy links compatible', () => {
    const routerSource = readSource(new URL('../../../components/layout/Router.tsx', import.meta.url));
    const siderSource = readSource(new URL('./SettingsSider.tsx', import.meta.url));
    const engineTabsSource = readSource(
      new URL('../../../components/settings/SettingsModal/contents/AgentModalContent.tsx', import.meta.url)
    );

    for (const path of [
      '/settings/execution-engines',
      '/settings/browser-use',
      '/settings/computer-use',
      '/settings/poi',
      '/settings/insights',
      '/settings/media',
      '/settings/open-capabilities',
      '/settings/cloud-login',
    ]) {
      expect(routerSource.includes(`path='${path}'`)).toBe(true);
    }

    expect(routerSource.includes("import('@renderer/pages/settings/AgentSettings')")).toBe(true);
    expect(routerSource.includes("to='/settings/execution-engines?tab=runtime'")).toBe(true);
    expect(routerSource.includes("to='/models?section=agents'")).toBe(false);
    expect(engineTabsSource.includes("key='runtime'")).toBe(true);
    expect(engineTabsSource.includes('<AgentRuntimeSettingsContent />')).toBe(true);
    expect(routerSource.includes("path='/settings/browser-use' element={<Navigate to='/settings/system'")).toBe(false);
    expect(routerSource.includes("path='/settings/computer-use' element={<Navigate to='/settings/system'")).toBe(false);
    expect(siderSource.includes("path: 'presets'")).toBe(true);
    expect(siderSource.includes("path: 'skills'")).toBe(true);
    expect(siderSource.includes("path: 'mcp'")).toBe(true);
    expect(routerSource.includes("path='/settings/presets'")).toBe(true);
    expect(routerSource.includes("path='/settings/skills'")).toBe(true);
    expect(routerSource.includes("path='/settings/mcp'")).toBe(true);
  });

  test('gates cloud account settings behind developer mode helpers', () => {
    const siderSource = readSource(new URL('./SettingsSider.tsx', import.meta.url));
    const pageWrapperSource = readSource(new URL('./SettingsPageWrapper.tsx', import.meta.url));
    const cloudLoginSource = readSource(new URL('../CloudLoginSettings.tsx', import.meta.url));
    const systemSource = readSource(
      new URL('../../../components/settings/SettingsModal/contents/SystemModalContent/index.tsx', import.meta.url)
    );

    expect(siderSource.includes('filterDeveloperGatedTabs')).toBe(true);
    expect(siderSource.includes("useConfig('system.developerMode')")).toBe(true);
    expect(pageWrapperSource.includes('filterDeveloperGatedTabs')).toBe(true);
    expect(cloudLoginSource.includes("useConfig('system.developerMode')")).toBe(true);
    expect(cloudLoginSource.includes("Navigate to='/settings/system'")).toBe(true);
    expect(systemSource.includes('DeveloperModeSetting')).toBe(true);
  });
});
