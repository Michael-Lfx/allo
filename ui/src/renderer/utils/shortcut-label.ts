import { hostOs, type HostOs } from '@/renderer/utils/platform';

export type { HostOs };

/** Accelerator tokens. `Mod` is Command on macOS and Ctrl on Windows/Linux. */
export type ShortcutPart = 'Mod' | 'Shift' | 'Alt' | 'Ctrl' | string;

const NAMED_KEYS = new Set(['Esc', 'Enter', 'Space', 'Delete', 'Backspace', 'Tab', 'Home', 'End']);

export function shortcutPartLabel(part: ShortcutPart, os: HostOs = hostOs()): string {
  if (part === 'Mod') return os === 'macos' ? '⌘' : 'Ctrl';
  if (part === 'Shift') return os === 'macos' ? '⇧' : 'Shift';
  if (part === 'Alt') return os === 'macos' ? '⌥' : 'Alt';
  if (part === 'Ctrl') return os === 'macos' ? '⌃' : 'Ctrl';
  return part;
}

export function shortcutKeyParts(parts: ShortcutPart[], os: HostOs = hostOs()): string[] {
  return normalizeShortcutParts(parts, os).map((part) => shortcutPartLabel(part, os));
}

/**
 * Compact accelerator for menus and tooltips.
 * macOS: `⌘C`, `⇧⌘Z`. Windows/Linux: `Ctrl+C`, `Ctrl+Shift+Z`.
 */
export function formatShortcut(parts: ShortcutPart[], os: HostOs = hostOs()): string {
  const ordered = shortcutKeyParts(parts, os);
  if (os === 'macos') return joinMacShortcut(ordered);
  return ordered.join('+');
}

export function formatShortcutChoice(options: ShortcutPart[][], os: HostOs = hostOs()): string {
  return options.map((parts) => formatShortcut(parts, os)).join(' / ');
}

function normalizeShortcutParts(parts: ShortcutPart[], os: HostOs): ShortcutPart[] {
  const flags = { mod: false, shift: false, alt: false, ctrl: false };
  const rest: ShortcutPart[] = [];
  for (const part of parts) {
    if (part === 'Mod') flags.mod = true;
    else if (part === 'Shift') flags.shift = true;
    else if (part === 'Alt') flags.alt = true;
    else if (part === 'Ctrl') flags.ctrl = true;
    else rest.push(part);
  }
  if (os === 'macos') {
    return [
      ...(flags.ctrl ? (['Ctrl'] as const) : []),
      ...(flags.alt ? (['Alt'] as const) : []),
      ...(flags.shift ? (['Shift'] as const) : []),
      ...(flags.mod ? (['Mod'] as const) : []),
      ...rest,
    ];
  }
  return [
    ...(flags.mod ? (['Mod'] as const) : []),
    ...(flags.ctrl ? (['Ctrl'] as const) : []),
    ...(flags.alt ? (['Alt'] as const) : []),
    ...(flags.shift ? (['Shift'] as const) : []),
    ...rest,
  ];
}

function joinMacShortcut(parts: string[]): string {
  let result = '';
  for (const part of parts) {
    if (!result) {
      result = part;
      continue;
    }
    const glue = isMacGlyph(part) || (isMacGlyph(result.slice(-1)) && isAtomicKey(part));
    result += glue ? part : `+${part}`;
  }
  return result;
}

function isMacGlyph(part: string): boolean {
  return part === '⌘' || part === '⇧' || part === '⌥' || part === '⌃';
}

function isAtomicKey(part: string): boolean {
  return part.length === 1 || NAMED_KEYS.has(part);
}
