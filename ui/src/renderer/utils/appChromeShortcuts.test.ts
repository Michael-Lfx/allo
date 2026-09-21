import { describe, expect, test } from 'bun:test';

import {
  APP_CHROME_SHORTCUT_BADGE_CLASS,
  appChromeShortcutTooltip,
  formatAppChromeShortcut,
  isAppChromeShortcutAvailable,
  listAppChromeShortcuts,
  listAppChromeShortcutsForSheet,
  matchAppChromeShortcut,
} from './appChromeShortcuts';

type ShortcutEvent = {
  key: string;
  code?: string;
  altKey?: boolean;
  metaKey?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
};

const chord = ({
  key,
  code = '',
  altKey = false,
  metaKey = false,
  ctrlKey = false,
  shiftKey = false,
}: ShortcutEvent) => ({ key, code, altKey, metaKey, ctrlKey, shiftKey });

const desktopWindows = { desktop: true, mobile: false, os: 'windows' as const };
const webWindows = { desktop: false, mobile: false, os: 'windows' as const };
const mobileWindows = { desktop: true, mobile: true, os: 'windows' as const };

describe('app chrome shortcut catalog', () => {
  test('lists every chrome shortcut with a group, host, and surface', () => {
    const ids = listAppChromeShortcuts().map((item) => item.id);
    expect(ids).toEqual([
      'historyBack',
      'historyForward',
      'sidebar',
      'settings',
      'newConversation',
      'search',
      'focusComposer',
      'cycleConversation',
      'cheatsheet',
    ]);
    expect(listAppChromeShortcuts().every((item) => item.group && item.host && item.surface && item.titleKey)).toBe(
      true
    );
  });

  test('keeps browser-conflicting chords desktop-only and marks display surfaces', () => {
    expect(isAppChromeShortcutAvailable('newConversation', desktopWindows)).toBe(true);
    expect(isAppChromeShortcutAvailable('newConversation', webWindows)).toBe(false);
    expect(isAppChromeShortcutAvailable('focusComposer', desktopWindows)).toBe(true);
    expect(isAppChromeShortcutAvailable('focusComposer', webWindows)).toBe(false);
    expect(isAppChromeShortcutAvailable('cycleConversation', desktopWindows)).toBe(true);
    expect(isAppChromeShortcutAvailable('sidebar', webWindows)).toBe(true);
    expect(isAppChromeShortcutAvailable('settings', webWindows)).toBe(true);
    expect(isAppChromeShortcutAvailable('sidebar', mobileWindows)).toBe(false);
    expect(listAppChromeShortcuts().find((item) => item.id === 'newConversation')?.surface).toBe('badge');
    expect(listAppChromeShortcuts().find((item) => item.id === 'focusComposer')?.surface).toBe('sheet-only');
    expect(listAppChromeShortcuts().find((item) => item.id === 'sidebar')?.surface).toBe('tooltip');
  });

  test('matches the planned chords and ignores extra modifiers', () => {
    expect(matchAppChromeShortcut(chord({ key: 't', ctrlKey: true }), desktopWindows)).toBe('newConversation');
    expect(matchAppChromeShortcut(chord({ key: 'l', metaKey: true }), { desktop: true, mobile: false, os: 'macos' })).toBe(
      'focusComposer'
    );
    expect(matchAppChromeShortcut(chord({ key: 'b', ctrlKey: true }), webWindows)).toBe('sidebar');
    expect(matchAppChromeShortcut(chord({ key: ',', code: 'Comma', ctrlKey: true }), webWindows)).toBe('settings');
    expect(matchAppChromeShortcut(chord({ key: '/', code: 'Slash', ctrlKey: true }), webWindows)).toBe('cheatsheet');
    expect(matchAppChromeShortcut(chord({ key: 'k', ctrlKey: true }), webWindows)).toBe('search');
    expect(matchAppChromeShortcut(chord({ key: 'Tab', ctrlKey: true }), desktopWindows)).toBe('cycleConversation');
    expect(matchAppChromeShortcut(chord({ key: 'Tab', ctrlKey: true, shiftKey: true }), desktopWindows)).toBe(
      'cycleConversation'
    );
    expect(matchAppChromeShortcut(chord({ key: 'ArrowLeft', altKey: true }), desktopWindows)).toBe('historyBack');
    expect(matchAppChromeShortcut(chord({ key: 't', ctrlKey: true }), webWindows)).toBeNull();
    expect(matchAppChromeShortcut(chord({ key: 'l', ctrlKey: true }), webWindows)).toBeNull();
    expect(matchAppChromeShortcut(chord({ key: 't', ctrlKey: true, shiftKey: true }), desktopWindows)).toBeNull();
    expect(matchAppChromeShortcut(chord({ key: 'b', ctrlKey: true }), mobileWindows)).toBeNull();
  });

  test('formats labels and keeps the sider badge class shared with search', () => {
    expect(formatAppChromeShortcut('newConversation', 'windows')).toBe('Ctrl+T');
    expect(formatAppChromeShortcut('newConversation', 'macos')).toBe('⌘T');
    expect(formatAppChromeShortcut('focusComposer', 'windows')).toBe('Ctrl+L');
    expect(formatAppChromeShortcut('sidebar', 'windows')).toBe('Ctrl+B');
    expect(formatAppChromeShortcut('settings', 'windows')).toBe('Ctrl+,');
    expect(formatAppChromeShortcut('cheatsheet', 'windows')).toBe('Ctrl+/');
    expect(formatAppChromeShortcut('search', 'macos')).toBe('⌘K');
    expect(formatAppChromeShortcut('historyBack', 'windows')).toBe('Alt+←');
    expect(formatAppChromeShortcut('historyForward', 'macos')).toBe('⌘]');
    expect(appChromeShortcutTooltip('设置', 'settings', 'windows')).toBe('设置 Ctrl+,');
    expect(APP_CHROME_SHORTCUT_BADGE_CLASS).toContain('font-mono');
    expect(APP_CHROME_SHORTCUT_BADGE_CLASS).toContain('ml-auto');
  });

  test('builds a cheatsheet without logout and hides desktop-only rows on the web', () => {
    const desktopIds = listAppChromeShortcutsForSheet(desktopWindows).map((item) => item.id);
    const webIds = listAppChromeShortcutsForSheet(webWindows).map((item) => item.id);
    expect(desktopIds).toEqual([
      'historyBack',
      'historyForward',
      'sidebar',
      'settings',
      'newConversation',
      'search',
      'focusComposer',
      'cycleConversation',
      'cheatsheet',
    ]);
    expect(webIds).toEqual(['historyBack', 'historyForward', 'sidebar', 'settings', 'search', 'cheatsheet']);
    expect(desktopIds.join(' ')).not.toContain('logout');
  });
});
