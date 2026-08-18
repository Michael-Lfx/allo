import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import type { IExtensionSettingsTab } from '@/common/adapter/ipcBridge';
import {
  buildSettingsNavigation,
  isSettingsNavItemActive,
  LEGACY_ANCHOR_REMAP,
  type SettingsNavGroupId,
  type SettingsNavItem,
} from './settingsNavigation';

const readSource = (url: URL) => readFileSync(url, 'utf8');

const groupLabels: Record<SettingsNavGroupId, string> = {
  application: 'Application',
  intelligence: 'Intelligence & Content',
  capabilities: 'Capabilities & Extensions',
  account: 'Account & Other',
};

const builtinItems: SettingsNavItem[] = [
  { id: 'system', path: 'system', label: 'System', icon: 'system', groupId: 'application' },
  { id: 'browser-use', path: 'browser-use', label: 'Browser Use', icon: 'browser-use', groupId: 'application' },
  { id: 'poi', path: 'poi', label: 'Topics', icon: 'poi', groupId: 'intelligence' },
  {
    id: 'capability-hub',
    path: 'presets',
    label: 'Capabilities',
    icon: 'presets',
    groupId: 'capabilities',
    activePaths: ['presets', 'skills', 'mcp', 'plugins'],
  },
  { id: 'about', path: 'about', label: 'About', icon: 'about', groupId: 'account' },
];

const extension = (id: string, position?: { relative_to: string; placement: 'before' | 'after' }): IExtensionSettingsTab =>
  ({ id, extension_name: 'sample', url: 'index.html', position } as unknown as IExtensionSettingsTab);

describe('settings navigation', () => {
  test('uses the fixed four-group order and suppresses empty group headers', () => {
    const groups = buildSettingsNavigation(builtinItems, [], groupLabels, () => {
      throw new Error('no extension expected');
    });

    expect(groups.map((group) => group.id)).toEqual(['application', 'intelligence', 'capabilities', 'account']);
    expect(groups[0].items.map((item) => item.id)).toEqual(['system', 'browser-use']);
  });

  test('keeps anchored extensions adjacent to their host and falls back to capabilities', () => {
    const groups = buildSettingsNavigation(
      builtinItems,
      [extension('before-skills', { relative_to: 'skills', placement: 'before' }), extension('retired-anchor', { relative_to: 'agent', placement: 'after' }), extension('no-anchor')],
      groupLabels,
      (tab, groupId) => ({
        id: tab.id,
        path: `ext/${tab.id}`,
        label: tab.id,
        icon: 'extension',
        groupId,
        isExtension: true,
      })
    );
    const capabilityItems = groups.find((group) => group.id === 'capabilities')?.items ?? [];

    expect(capabilityItems.map((item) => item.id)).toEqual([
      'before-skills',
      'capability-hub',
      'retired-anchor',
      'no-anchor',
    ]);
    expect(LEGACY_ANCHOR_REMAP.agent).toBe('execution-engines');
    expect(LEGACY_ANCHOR_REMAP.skills).toBe('capability-hub');
  });

  test('capability hub nav item stays active across presets, skills, MCP, and plugins', () => {
    const item = {
      path: 'presets',
      activePaths: ['presets', 'skills', 'mcp', 'plugins'],
    };

    expect(isSettingsNavItemActive('/settings/presets', item)).toBe(true);
    expect(isSettingsNavItemActive('/settings/skills', item)).toBe(true);
    expect(isSettingsNavItemActive('/settings/mcp', item)).toBe(true);
    expect(isSettingsNavItemActive('/settings/plugins', item)).toBe(true);
    expect(isSettingsNavItemActive('/settings/system', item)).toBe(false);
  });

  test('desktop and narrow-window shells consume one shared navigation model', () => {
    const siderSource = readSource(new URL('./SettingsSider.tsx', import.meta.url));
    const wrapperSource = readSource(new URL('./SettingsPageWrapper.tsx', import.meta.url));
    const navigationSource = readSource(new URL('./settingsNavigation.ts', import.meta.url));

    expect(siderSource).toContain('useSettingsNavigation');
    expect(wrapperSource).toContain('useSettingsNavigation');
    expect(navigationSource).toContain('filterDeveloperGatedTabs');
    expect(navigationSource).toContain("'capabilities'");
  });

  test('desktop navigation uses accessible buttons and a local indicator', () => {
    const siderSource = readSource(new URL('./SettingsSider.tsx', import.meta.url));

    expect(siderSource).toContain("type='button'");
    expect(siderSource).toContain("aria-current={selected ? 'page' : undefined}");
    expect(siderSource).toContain('useSlidingSelectionIndicator');
    expect(siderSource).toContain('scrollIntoView({ block: \'nearest\' })');
  });

  test('uses typed, resolvable built-in group labels rather than exposing raw i18n keys', () => {
    const navigationSource = readSource(new URL('./settingsNavigation.ts', import.meta.url));
    const zh = JSON.parse(readSource(new URL('../../../services/i18n/locales/zh-CN/settings.json', import.meta.url))) as {
      groupApp: string;
      groupIntelligenceContent: string;
      groupCapabilityExtensions: string;
      groupAccountOther: string;
    };
    const en = JSON.parse(readSource(new URL('../../../services/i18n/locales/en-US/settings.json', import.meta.url))) as typeof zh;

    expect(navigationSource).toContain("application: 'settings.groupApp'");
    expect(navigationSource).toContain("labelKey: 'settings.capabilityHub.navLabel'");
    expect(navigationSource).toContain('I18nKey');
    expect(navigationSource).not.toContain('settings.groupApplication');
    for (const label of [
      zh.groupApp,
      zh.groupIntelligenceContent,
      zh.groupCapabilityExtensions,
      zh.groupAccountOther,
      en.groupApp,
      en.groupIntelligenceContent,
      en.groupCapabilityExtensions,
      en.groupAccountOther,
    ]) {
      expect(label).not.toMatch(/^settings\./);
    }
  });
});
