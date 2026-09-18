import { describe, expect, test } from 'bun:test';

import { formatShortcut, formatShortcutChoice, shortcutKeyParts } from './shortcut-label';

describe('formatShortcut', () => {
  test('uses Command glyphs on the macOS package', () => {
    expect(formatShortcut(['Mod', 'C'], 'macos')).toBe('⌘C');
    expect(formatShortcut(['Shift', 'Mod', 'Z'], 'macos')).toBe('⇧⌘Z');
    expect(formatShortcut(['Shift', 'Mod', 'F'], 'macos')).toBe('⇧⌘F');
    expect(formatShortcut(['Mod', '拖动'], 'macos')).toBe('⌘+拖动');
  });

  test('uses Ctrl+ on Windows and Linux packages', () => {
    expect(formatShortcut(['Mod', 'C'], 'windows')).toBe('Ctrl+C');
    expect(formatShortcut(['Shift', 'Mod', 'Z'], 'windows')).toBe('Ctrl+Shift+Z');
    expect(formatShortcut(['Mod', 'C'], 'linux')).toBe('Ctrl+C');
    expect(formatShortcut(['Shift', 'Mod', 'F'], 'linux')).toBe('Ctrl+Shift+F');
  });

  test('joins alternate accelerators', () => {
    expect(formatShortcutChoice([['Shift', 'Mod', 'Z'], ['Mod', 'Y']], 'windows')).toBe('Ctrl+Shift+Z / Ctrl+Y');
    expect(formatShortcutChoice([['Shift', 'Mod', 'Z'], ['Mod', 'Y']], 'macos')).toBe('⇧⌘Z / ⌘Y');
  });

  test('splits keys for the shortcut modal', () => {
    expect(shortcutKeyParts(['Mod', 'F'], 'windows')).toEqual(['Ctrl', 'F']);
    expect(shortcutKeyParts(['Shift', 'Mod', 'F'], 'macos')).toEqual(['⇧', '⌘', 'F']);
  });
});
