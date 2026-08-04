

import { afterEach, describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { isPreference, resolveTheme } from './useTheme';

const DARK_QUERY = '(prefers-color-scheme: dark)';

const originalWindow = globalThis.window;

const mockMatchMedia = (dark: boolean) =>
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    writable: true,
    value: {
      matchMedia: (query: string) => ({ matches: query === DARK_QUERY ? dark : false }),
    },
  });

const clearWindow = () =>
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    writable: true,
    value: undefined,
  });

const restoreWindow = () =>
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    writable: true,
    value: originalWindow,
  });

afterEach(() => {
  restoreWindow();
});

describe('resolveTheme', () => {
  test('passes explicit light/dark through unchanged', () => {
    expect(resolveTheme('light')).toBe('light');
    expect(resolveTheme('dark')).toBe('dark');
  });

  test('resolves the system preference from prefers-color-scheme', () => {
    mockMatchMedia(true);
    expect(resolveTheme('system')).toBe('dark');
    mockMatchMedia(false);
    expect(resolveTheme('system')).toBe('light');
  });

  test('falls back to light when no window/matchMedia is available', () => {
    clearWindow();
    expect(resolveTheme('system')).toBe('light');
  });
});

describe('isPreference', () => {
  test('accepts the three valid preferences', () => {
    expect(isPreference('light')).toBe(true);
    expect(isPreference('dark')).toBe(true);
    expect(isPreference('system')).toBe(true);
  });

  test('rejects empty, unknown, and case-mismatched values', () => {
    expect(isPreference('')).toBe(false);
    expect(isPreference('auto')).toBe(false);
    expect(isPreference('LIGHT')).toBe(false);
    expect(isPreference('darkk')).toBe(false);
  });
});

// The inline boot scripts in both entry HTML files must resolve a persisted
// 'system' preference to a concrete light/dark before writing data-theme —
// otherwise the app would paint with an invalid data-theme="system" on load.
describe('theme FOUC boot script', () => {
  const rendererHtml = readFileSync(new URL('../../index.html', import.meta.url), 'utf8');
  const webHtml = readFileSync(new URL('../../../../index.html', import.meta.url), 'utf8');

  for (const [name, html] of [
    ['renderer entry', rendererHtml],
    ['web entry', webHtml],
  ] as const) {
    test(`${name} resolves a stored 'system' preference via prefers-color-scheme`, () => {
      expect(html.includes("=== 'system'")).toBe(true);
      expect(html.includes('prefers-color-scheme: dark')).toBe(true);
    });

    test(`${name} normalizes a missing/invalid preference to 'system' before resolving`, () => {
      // Fresh installs (and legacy invalid values) must fall back to 'system'
      // in BOTH the head (data-theme) and body (arco-theme) scripts — otherwise
      // the hardcoded data-theme="light" survives and dark-OS users flash.
      const normalizeCount = html.split("= 'system';").length - 1;
      expect(normalizeCount).toBeGreaterThanOrEqual(2);
    });
  }
});

// The switcher presents options in system → light → dark order (system first
// because fresh installs default to it). The sliding capsule and keyboard nav
// are index-driven, so this order is the single source of truth.
describe('ThemeSwitcher option order', () => {
  const source = readFileSync(
    new URL('../../components/settings/ThemeSwitcher.tsx', import.meta.url),
    'utf8'
  );

  test('orders options system, light, dark', () => {
    const systemIdx = source.indexOf("value: 'system'");
    const lightIdx = source.indexOf("value: 'light'");
    const darkIdx = source.indexOf("value: 'dark'");
    expect(systemIdx).toBeGreaterThanOrEqual(0);
    expect(systemIdx).toBeLessThan(lightIdx);
    expect(lightIdx).toBeLessThan(darkIdx);
  });
});
