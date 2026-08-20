/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { type IExtensionSettingsTab } from '@/common/adapter/ipcBridge';
import { filterDeveloperGatedTabs } from '@/common/config/developerMode';
import { useDeveloperModeGate } from '@/renderer/hooks/config/useDeveloperModeGate';
import { useExtI18n } from '@/renderer/hooks/system/useExtI18n';
import { useExtensionSettingsTabs } from '@/renderer/hooks/system/useExtensionSettingsTabs';
import type { I18nKey } from '@/renderer/services/i18n';
import { resolveExtensionAssetUrl } from '@/renderer/utils/platform';

export type SettingsNavGroupId = 'application' | 'intelligence' | 'capabilities' | 'account';

export type SettingsNavIcon =
  | 'system'
  | 'browser-use'
  | 'computer-use'
  | 'poi'
  | 'learning'
  | 'insights'
  | 'moa'
  | 'media'
  | 'presets'
  | 'skills'
  | 'mcp'
  | 'cloud-login'
  | 'about';

export type SettingsNavItem = {
  id: string;
  label: string;
  path: string;
  groupId: SettingsNavGroupId;
  icon: SettingsNavIcon | 'extension';
  iconUrl?: string;
  isExtension?: boolean;
  /** Extra path segments that should keep this item selected (capability hub). */
  activePaths?: string[];
};

export type SettingsNavGroup = {
  id: SettingsNavGroupId;
  label: string;
  items: SettingsNavItem[];
};

type SettingsNavBuiltin = Omit<SettingsNavItem, 'label'> & { labelKey: I18nKey };

/** Legacy placements supported by released extensions. */
export const LEGACY_ANCHOR_REMAP: Record<string, string> = {
  agent: 'execution-engines',
  'agent-runtime': 'execution-engines',
  'skills-hub': 'capability-hub',
  tools: 'capability-hub',
  skills: 'capability-hub',
  presets: 'capability-hub',
  mcp: 'capability-hub',
};

export const SETTINGS_NAV_GROUP_ORDER: SettingsNavGroupId[] = [
  'application',
  'intelligence',
  'capabilities',
  'account',
];

const GROUP_LABEL_KEYS: Record<SettingsNavGroupId, I18nKey> = {
  application: 'settings.groupApp',
  intelligence: 'settings.groupIntelligenceContent',
  capabilities: 'settings.groupCapabilityExtensions',
  account: 'settings.groupAccountOther',
};

const BUILTIN_NAVIGATION: SettingsNavBuiltin[] = [
  { id: 'system', path: 'system', groupId: 'application', icon: 'system', labelKey: 'settings.system' },
  { id: 'browser-use', path: 'browser-use', groupId: 'application', icon: 'browser-use', labelKey: 'settings.browserUseNav' },
  { id: 'computer-use', path: 'computer-use', groupId: 'application', icon: 'computer-use', labelKey: 'settings.computerUseNav' },
  { id: 'poi', path: 'poi', groupId: 'intelligence', icon: 'poi', labelKey: 'settings.poiNav' },
  { id: 'learning', path: 'learning', groupId: 'intelligence', icon: 'learning', labelKey: 'settings.learningNav' },
  { id: 'insights', path: 'insights', groupId: 'intelligence', icon: 'insights', labelKey: 'settings.insightsNav' },
  { id: 'moa', path: 'moa', groupId: 'intelligence', icon: 'moa', labelKey: 'settings.moaNav' },
  { id: 'media', path: 'media', groupId: 'intelligence', icon: 'media', labelKey: 'settings.mediaNav' },
  {
    id: 'capability-hub',
    path: 'presets',
    groupId: 'capabilities',
    icon: 'presets',
    labelKey: 'settings.capabilityHub.navLabel',
    activePaths: ['presets', 'skills', 'mcp', 'plugins'],
  },
  { id: 'cloud-login', path: 'cloud-login', groupId: 'account', icon: 'cloud-login', labelKey: 'settings.cloudLoginNav' },
  { id: 'about', path: 'about', groupId: 'account', icon: 'about', labelKey: 'settings.about' },
];

const resolvePlacement = (
  tab: IExtensionSettingsTab,
  builtinById: Map<string, SettingsNavItem>
): string | undefined => {
  const rawAnchor = tab.position?.relative_to;
  const anchor = rawAnchor ? (LEGACY_ANCHOR_REMAP[rawAnchor] ?? rawAnchor) : undefined;
  const anchoredBuiltin = anchor ? builtinById.get(anchor) : undefined;

  // Missing, retired, or currently developer-gated anchors are placed at the
  // end of the extension group instead of leaking into an unrelated group.
  return anchoredBuiltin ? anchor : undefined;
};

/**
 * Combines the fixed four-group information architecture with extension tabs.
 * It stays free of rendering details so desktop and narrow-window navigation
 * always use the same ordering, gates, and anchor compatibility rules.
 */
export function buildSettingsNavigation(
  builtinItems: SettingsNavItem[],
  extensionTabs: IExtensionSettingsTab[],
  groupLabels: Record<SettingsNavGroupId, string>,
  toExtensionItem: (tab: IExtensionSettingsTab, groupId: SettingsNavGroupId) => SettingsNavItem
): SettingsNavGroup[] {
  const builtinById = new Map(builtinItems.map((item) => [item.id, item]));
  const before = new Map<string, IExtensionSettingsTab[]>();
  const after = new Map<string, IExtensionSettingsTab[]>();
  const capabilityTail: IExtensionSettingsTab[] = [];

  for (const tab of extensionTabs) {
    const anchor = resolvePlacement(tab, builtinById);
    if (!anchor || !tab.position) {
      capabilityTail.push(tab);
      continue;
    }

    const map = tab.position.placement === 'before' ? before : after;
    const entries = map.get(anchor) ?? [];
    entries.push(tab);
    map.set(anchor, entries);

  }

  const groups = SETTINGS_NAV_GROUP_ORDER.map((id) => ({
    id,
    label: groupLabels[id],
    items: [] as SettingsNavItem[],
  }));
  const groupsById = new Map(groups.map((group) => [group.id, group]));

  for (const builtin of builtinItems) {
    const group = groupsById.get(builtin.groupId);
    if (!group) continue;

    const beforeTabs = before.get(builtin.id) ?? [];
    group.items.push(...beforeTabs.map((tab) => toExtensionItem(tab, builtin.groupId)));
    group.items.push(builtin);
    const afterTabs = after.get(builtin.id) ?? [];
    group.items.push(...afterTabs.map((tab) => toExtensionItem(tab, builtin.groupId)));
  }

  const capabilityGroup = groupsById.get('capabilities');
  if (capabilityGroup) {
    capabilityGroup.items.push(...capabilityTail.map((tab) => toExtensionItem(tab, 'capabilities')));
  }

  return groups.filter((group) => group.items.length > 0);
}

export const isSettingsNavItemActive = (
  pathname: string,
  item: Pick<SettingsNavItem, 'path' | 'activePaths'>
): boolean => {
  const paths = item.activePaths?.length ? item.activePaths : [item.path];
  return paths.some((path) => {
    const target = path.startsWith('/') ? path : `/settings/${path}`;
    return pathname === target || pathname.startsWith(`${target}/`);
  });
};

/**
 * Shared navigation data for both settings shells. No renderer owns a second
 * hard-coded menu definition.
 */
export function useSettingsNavigation(): ReturnType<typeof useExtensionSettingsTabs> & {
  groups: SettingsNavGroup[];
} {
  const { t } = useTranslation();
  const { active: developerMode } = useDeveloperModeGate();
  const { resolveExtTabName } = useExtI18n();
  const extensionState = useExtensionSettingsTabs();

  const groups = useMemo(() => {
    const visibleBuiltinIds = new Set(
      filterDeveloperGatedTabs(
        BUILTIN_NAVIGATION.map((item) => item.id),
        developerMode
      )
    );
    const builtinItems = BUILTIN_NAVIGATION.filter((item) => visibleBuiltinIds.has(item.id)).map(
      ({ labelKey, ...item }) => ({ ...item, label: t(labelKey) })
    );
    const groupLabels = Object.fromEntries(
      SETTINGS_NAV_GROUP_ORDER.map((groupId) => [groupId, t(GROUP_LABEL_KEYS[groupId])])
    ) as Record<SettingsNavGroupId, string>;

    return buildSettingsNavigation(
      builtinItems,
      extensionState.tabs,
      groupLabels,
      (tab, groupId): SettingsNavItem => ({
        id: `ext-${tab.id}`,
        label: resolveExtTabName(tab),
        path: `ext/${tab.id}`,
        groupId,
        icon: 'extension',
        iconUrl: resolveExtensionAssetUrl(tab.icon) ?? tab.icon,
        isExtension: true,
      })
    );
  }, [developerMode, extensionState.tabs, resolveExtTabName, t]);

  return { ...extensionState, groups };
}
