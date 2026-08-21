
import type { ITheme } from '@xterm/xterm';

const DARK_ANSI: ITheme = {
  black: '#3f4451',
  red: '#e06c75',
  green: '#98c379',
  yellow: '#e5c07b',
  blue: '#61afef',
  magenta: '#c678dd',
  cyan: '#56b6c2',
  white: '#d7dae0',
  brightBlack: '#4f5666',
  brightRed: '#ff7b86',
  brightGreen: '#a8d98a',
  brightYellow: '#f0cd8b',
  brightBlue: '#74bbff',
  brightMagenta: '#d68ee8',
  brightCyan: '#66c6d2',
  brightWhite: '#ffffff',
};

const LIGHT_ANSI: ITheme = {
  black: '#1d2129',
  red: '#c42b31',
  green: '#0d7a3e',
  yellow: '#9a6700',
  blue: '#165dff',
  magenta: '#7d4ead',
  cyan: '#087990',
  white: '#454d5f',
  brightBlack: '#86909c',
  brightRed: '#f53f3f',
  brightGreen: '#00b42a',
  brightYellow: '#ff7d00',
  brightBlue: '#4080ff',
  brightMagenta: '#c678dd',
  brightCyan: '#0fc6c2',
  brightWhite: '#1d2129',
};

function readCssColor(name: string, fallback: string): string {
  if (typeof document === 'undefined') return fallback;
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value || fallback;
}

export function isDarkTerminalTheme(): boolean {
  if (typeof document === 'undefined') return false;
  return document.documentElement.getAttribute('data-theme') === 'dark';
}

/**
 * xterm canvas theme that follows the app light/dark (and color-scheme) tokens.
 * Background matches `--terminal-surface-bg` → `--bg-1`.
 */
export function resolveTerminalTheme(): ITheme {
  const dark = isDarkTerminalTheme();
  const background = readCssColor('--terminal-surface-bg', readCssColor('--bg-1', dark ? '#1a1a1a' : '#f9fafb'));
  const foreground = readCssColor('--text-primary', dark ? '#ffffff' : '#000000');
  return {
    ...(dark ? DARK_ANSI : LIGHT_ANSI),
    background,
    foreground,
    cursor: foreground,
    cursorAccent: background,
    selectionBackground: dark ? 'rgba(122,131,178,0.40)' : 'rgba(22,93,255,0.28)',
    selectionForeground: dark ? '#ffffff' : '#000000',
  };
}

/** Typography options applied to the xterm Terminal constructor. */
export const TERMINAL_TYPOGRAPHY = {
  fontSize: 13,
  lineHeight: 1.25,
  letterSpacing: 0,
  fontWeight: 400 as const,
  fontWeightBold: 600 as const,
};
