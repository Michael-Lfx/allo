import {
  formatHistoryBackShortcut,
  formatHistoryForwardShortcut,
  historyBackShortcutParts,
  historyForwardShortcutParts,
  isHistoryBackShortcut,
  isHistoryForwardShortcut,
  type HistoryShortcutEvent,
} from '@/renderer/utils/historyNavigationShortcut';
import type { I18nKey } from '@/renderer/services/i18n/i18n-keys';
import { hostOs, type HostOs } from '@/renderer/utils/platform';
import { formatShortcut, type ShortcutPart } from '@/renderer/utils/shortcut-label';

export type AppChromeShortcutId =
  | 'historyBack'
  | 'historyForward'
  | 'sidebar'
  | 'settings'
  | 'newConversation'
  | 'search'
  | 'focusComposer'
  | 'cycleConversation'
  | 'cheatsheet';

export type AppChromeShortcutHost = 'desktop' | 'all';
export type AppChromeShortcutSurface = 'badge' | 'tooltip' | 'sheet-only';
export type AppChromeShortcutGroup = 'navigation' | 'conversation' | 'help';

export type AppChromeShortcutEvent = Pick<KeyboardEvent, 'key' | 'altKey' | 'metaKey' | 'ctrlKey' | 'shiftKey'> & {
  code?: string;
};

export type AppChromeRuntime = {
  desktop: boolean;
  mobile: boolean;
  os?: HostOs;
};

export type AppChromeShortcutDefinition = {
  id: AppChromeShortcutId;
  group: AppChromeShortcutGroup;
  host: AppChromeShortcutHost;
  surface: AppChromeShortcutSurface;
  titleKey: I18nKey;
  parts: (os?: HostOs) => ShortcutPart[];
  matches: (event: AppChromeShortcutEvent, os: HostOs) => boolean;
};

export const APP_CHROME_SHORTCUT_BADGE_CLASS =
  'collapsed-hidden ml-auto text-10px text-t-tertiary px-5px py-2px rd-4px border border-solid border-[var(--color-border-2)] bg-fill-1 font-mono leading-none select-none';

const isMod = (event: AppChromeShortcutEvent): boolean =>
  (event.metaKey || event.ctrlKey) && !event.altKey;

const isPlainMod = (event: AppChromeShortcutEvent): boolean => isMod(event) && !event.shiftKey;

const keyOf = (event: AppChromeShortcutEvent): string => event.key.toLowerCase();

const APP_CHROME_SHORTCUTS: readonly AppChromeShortcutDefinition[] = [
  {
    id: 'historyBack',
    group: 'navigation',
    host: 'all',
    surface: 'tooltip',
    titleKey: 'common.shortcuts.historyBack',
    parts: (os = hostOs()) => historyBackShortcutParts(os),
    matches: (event, os) => isHistoryBackShortcut(event as HistoryShortcutEvent, os),
  },
  {
    id: 'historyForward',
    group: 'navigation',
    host: 'all',
    surface: 'tooltip',
    titleKey: 'common.shortcuts.historyForward',
    parts: (os = hostOs()) => historyForwardShortcutParts(os),
    matches: (event, os) => isHistoryForwardShortcut(event as HistoryShortcutEvent, os),
  },
  {
    id: 'sidebar',
    group: 'navigation',
    host: 'all',
    surface: 'tooltip',
    titleKey: 'common.shortcuts.sidebar',
    parts: () => ['Mod', 'B'],
    matches: (event) => isPlainMod(event) && keyOf(event) === 'b',
  },
  {
    id: 'settings',
    group: 'navigation',
    host: 'all',
    surface: 'tooltip',
    titleKey: 'common.shortcuts.settings',
    parts: () => ['Mod', ','],
    matches: (event) => isPlainMod(event) && (event.key === ',' || event.code === 'Comma'),
  },
  {
    id: 'newConversation',
    group: 'conversation',
    host: 'desktop',
    surface: 'badge',
    titleKey: 'common.shortcuts.newConversation',
    parts: () => ['Mod', 'T'],
    matches: (event) => isPlainMod(event) && keyOf(event) === 't',
  },
  {
    id: 'search',
    group: 'conversation',
    host: 'all',
    surface: 'badge',
    titleKey: 'common.shortcuts.search',
    parts: () => ['Mod', 'K'],
    matches: (event) => isPlainMod(event) && keyOf(event) === 'k',
  },
  {
    id: 'focusComposer',
    group: 'conversation',
    host: 'desktop',
    surface: 'sheet-only',
    titleKey: 'common.shortcuts.focusComposer',
    parts: () => ['Mod', 'L'],
    matches: (event) => isPlainMod(event) && keyOf(event) === 'l',
  },
  {
    id: 'cycleConversation',
    group: 'conversation',
    host: 'desktop',
    surface: 'sheet-only',
    titleKey: 'common.shortcuts.cycleConversation',
    parts: () => ['Ctrl', 'Tab'],
    matches: (event) => event.ctrlKey && !event.metaKey && !event.altKey && event.key === 'Tab',
  },
  {
    id: 'cheatsheet',
    group: 'help',
    host: 'all',
    surface: 'sheet-only',
    titleKey: 'common.shortcuts.cheatsheet',
    parts: () => ['Mod', '/'],
    matches: (event) => isMod(event) && (event.key === '/' || event.key === '?' || event.code === 'Slash'),
  },
];

const resolveOs = (runtime: AppChromeRuntime): HostOs => runtime.os ?? hostOs();

export function listAppChromeShortcuts(): readonly AppChromeShortcutDefinition[] {
  return APP_CHROME_SHORTCUTS;
}

export function getAppChromeShortcut(id: AppChromeShortcutId): AppChromeShortcutDefinition {
  const shortcut = APP_CHROME_SHORTCUTS.find((item) => item.id === id);
  if (!shortcut) {
    throw new Error(`Unknown app chrome shortcut: ${id}`);
  }
  return shortcut;
}

export function isAppChromeShortcutAvailable(id: AppChromeShortcutId, runtime: AppChromeRuntime): boolean {
  if (runtime.mobile) {
    return false;
  }
  const shortcut = getAppChromeShortcut(id);
  return shortcut.host === 'all' || runtime.desktop;
}

export function listAppChromeShortcutsForSheet(runtime: AppChromeRuntime): AppChromeShortcutDefinition[] {
  return APP_CHROME_SHORTCUTS.filter((item) => {
    if (item.host === 'desktop' && !runtime.desktop) {
      return false;
    }
    return true;
  });
}

export function matchAppChromeShortcut(
  event: AppChromeShortcutEvent,
  runtime: AppChromeRuntime
): AppChromeShortcutId | null {
  if (runtime.mobile) {
    return null;
  }
  const os = resolveOs(runtime);
  for (const shortcut of APP_CHROME_SHORTCUTS) {
    if (!isAppChromeShortcutAvailable(shortcut.id, runtime)) {
      continue;
    }
    if (shortcut.matches(event, os)) {
      return shortcut.id;
    }
  }
  return null;
}

export function formatAppChromeShortcut(id: AppChromeShortcutId, os: HostOs = hostOs()): string {
  if (id === 'historyBack') return formatHistoryBackShortcut(os);
  if (id === 'historyForward') return formatHistoryForwardShortcut(os);
  return formatShortcut(getAppChromeShortcut(id).parts(os), os);
}

export function appChromeShortcutTooltip(
  label: string,
  id: AppChromeShortcutId,
  os: HostOs = hostOs()
): string {
  return `${label} ${formatAppChromeShortcut(id, os)}`;
}
