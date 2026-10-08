/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./terminalTheme.ts', import.meta.url), 'utf8');
const xtermSource = readFileSync(new URL('./XtermView.tsx', import.meta.url), 'utf8');
const defaultScheme = readFileSync(
  new URL('../../styles/themes/default-color-scheme.css', import.meta.url),
  'utf8'
);

describe('terminal canvas theme', () => {
  test('resolves xterm colors from the app surface tokens instead of a constant-dark canvas', () => {
    expect(source.includes('export function resolveTerminalTheme')).toBe(true);
    expect(source.includes("readCssColor('--terminal-surface-bg'")).toBe(true);
    expect(source.includes("document.documentElement.getAttribute('data-theme') === 'dark'")).toBe(true);
    expect(xtermSource.includes('resolveTerminalTheme()')).toBe(true);
    expect(xtermSource.includes("attributeFilter: ['data-theme', 'data-color-scheme', 'data-wallpaper']")).toBe(true);
    expect(xtermSource.includes('allowTransparency: true')).toBe(true);
    expect(xtermSource.includes('isWallpaperTerminalTheme()')).toBe(true);
    expect(source.includes("getAttribute('data-wallpaper') === 'on'")).toBe(true);
    expect(source.includes("'rgba(0,0,0,0)'")).toBe(true);
  });

  test('default scheme maps the terminal card to the page background in both modes', () => {
    expect(defaultScheme.includes('--terminal-surface-bg: var(--bg-1);')).toBe(true);
    expect(defaultScheme.includes('--terminal-surface-bg: #1b1d23;')).toBe(false);
  });
});
