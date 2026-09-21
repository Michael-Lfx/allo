import { hostOs, type HostOs } from '@/renderer/utils/platform';
import { formatShortcut, type ShortcutPart } from '@/renderer/utils/shortcut-label';

export type HistoryShortcutEvent = Pick<KeyboardEvent, 'key' | 'altKey' | 'metaKey' | 'ctrlKey' | 'shiftKey'>;

const hasExtraModifiers = (event: HistoryShortcutEvent): boolean =>
  event.shiftKey || event.ctrlKey || (event.metaKey && event.altKey);

export function historyBackShortcutParts(os: HostOs = hostOs()): ShortcutPart[] {
  return os === 'macos' ? ['Mod', '['] : ['Alt', '←'];
}

export function historyForwardShortcutParts(os: HostOs = hostOs()): ShortcutPart[] {
  return os === 'macos' ? ['Mod', ']'] : ['Alt', '→'];
}

export function formatHistoryBackShortcut(os: HostOs = hostOs()): string {
  return formatShortcut(historyBackShortcutParts(os), os);
}

export function formatHistoryForwardShortcut(os: HostOs = hostOs()): string {
  return formatShortcut(historyForwardShortcutParts(os), os);
}

export function isHistoryBackShortcut(event: HistoryShortcutEvent, os: HostOs = hostOs()): boolean {
  if (event.key === 'BrowserBack') return true;
  if (hasExtraModifiers(event)) return false;
  if (os === 'macos') {
    return event.metaKey && !event.altKey && event.key === '[';
  }
  return event.altKey && !event.metaKey && event.key === 'ArrowLeft';
}

export function isHistoryForwardShortcut(event: HistoryShortcutEvent, os: HostOs = hostOs()): boolean {
  if (event.key === 'BrowserForward') return true;
  if (hasExtraModifiers(event)) return false;
  if (os === 'macos') {
    return event.metaKey && !event.altKey && event.key === ']';
  }
  return event.altKey && !event.metaKey && event.key === 'ArrowRight';
}
