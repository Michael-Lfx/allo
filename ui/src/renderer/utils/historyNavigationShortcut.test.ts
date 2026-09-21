import { describe, expect, test } from 'bun:test';

import {
  formatHistoryBackShortcut,
  formatHistoryForwardShortcut,
  historyBackShortcutParts,
  historyForwardShortcutParts,
  isHistoryBackShortcut,
  isHistoryForwardShortcut,
} from './historyNavigationShortcut';

type ShortcutEvent = {
  key: string;
  altKey?: boolean;
  metaKey?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
};

const chord = ({
  key,
  altKey = false,
  metaKey = false,
  ctrlKey = false,
  shiftKey = false,
}: ShortcutEvent) => ({ key, altKey, metaKey, ctrlKey, shiftKey });

describe('history navigation shortcuts', () => {
  test('uses Alt+arrows on Windows and Linux', () => {
    expect(isHistoryBackShortcut(chord({ key: 'ArrowLeft', altKey: true }), 'windows')).toBe(true);
    expect(isHistoryForwardShortcut(chord({ key: 'ArrowRight', altKey: true }), 'linux')).toBe(true);
    expect(historyBackShortcutParts('windows')).toEqual(['Alt', '←']);
    expect(historyForwardShortcutParts('linux')).toEqual(['Alt', '→']);
    expect(formatHistoryBackShortcut('windows')).toBe('Alt+←');
    expect(formatHistoryForwardShortcut('linux')).toBe('Alt+→');
  });

  test('uses Command brackets on macOS so Option+arrow keeps word movement', () => {
    expect(isHistoryBackShortcut(chord({ key: '[', metaKey: true }), 'macos')).toBe(true);
    expect(isHistoryForwardShortcut(chord({ key: ']', metaKey: true }), 'macos')).toBe(true);
    expect(isHistoryBackShortcut(chord({ key: 'ArrowLeft', altKey: true }), 'macos')).toBe(false);
    expect(isHistoryForwardShortcut(chord({ key: 'ArrowRight', altKey: true }), 'macos')).toBe(false);
    expect(historyBackShortcutParts('macos')).toEqual(['Mod', '[']);
    expect(historyForwardShortcutParts('macos')).toEqual(['Mod', ']']);
    expect(formatHistoryBackShortcut('macos')).toBe('⌘[');
    expect(formatHistoryForwardShortcut('macos')).toBe('⌘]');
  });

  test('ignores bare arrows, word-jump chords, and extra modifiers', () => {
    expect(isHistoryBackShortcut(chord({ key: 'ArrowLeft' }), 'windows')).toBe(false);
    expect(isHistoryBackShortcut(chord({ key: 'ArrowLeft', ctrlKey: true }), 'windows')).toBe(false);
    expect(isHistoryBackShortcut(chord({ key: 'ArrowLeft', altKey: true, shiftKey: true }), 'windows')).toBe(false);
    expect(isHistoryBackShortcut(chord({ key: '[', metaKey: true, altKey: true }), 'macos')).toBe(false);
    expect(isHistoryForwardShortcut(chord({ key: 'ArrowRight', altKey: true, ctrlKey: true }), 'windows')).toBe(false);
    expect(isHistoryForwardShortcut(chord({ key: ']', metaKey: true, shiftKey: true }), 'macos')).toBe(false);
  });

  test('accepts dedicated browser back and forward keys', () => {
    expect(isHistoryBackShortcut(chord({ key: 'BrowserBack' }), 'windows')).toBe(true);
    expect(isHistoryForwardShortcut(chord({ key: 'BrowserForward' }), 'macos')).toBe(true);
  });
});
