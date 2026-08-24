/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import enNotifications from './locales/en-US/notifications.json';
import zhNotifications from './locales/zh-CN/notifications.json';

type LocaleJson = Record<string, unknown>;

const flattenKeys = (locale: LocaleJson, prefix = ''): string[] =>
  Object.entries(locale).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    if (value && typeof value === 'object') return flattenKeys(value as LocaleJson, path);
    return [path];
  });

const getLocaleValue = (locale: LocaleJson, key: string): unknown => {
  let cursor: unknown = locale;
  for (const segment of key.split('.')) {
    if (!cursor || typeof cursor !== 'object' || !Object.prototype.hasOwnProperty.call(cursor, segment)) {
      return undefined;
    }
    cursor = (cursor as LocaleJson)[segment];
  }
  return cursor;
};

describe('notifications locale bundle', () => {
  test('en-US and zh-CN expose the same key set', () => {
    expect(flattenKeys(zhNotifications).sort()).toEqual(flattenKeys(enNotifications).sort());
  });

  test('covers the counter, collapse and close copy the stack renders', () => {
    for (const key of ['more', 'collapse', 'close']) {
      expect(typeof getLocaleValue(enNotifications, key)).toBe('string');
      expect(typeof getLocaleValue(zhNotifications, key)).toBe('string');
    }
  });

  test('the more counter copy carries the count placeholder in both locales', () => {
    expect(getLocaleValue(enNotifications, 'more')).toContain('{{count}}');
    expect(getLocaleValue(zhNotifications, 'more')).toContain('{{count}}');
  });

  test('every notification level has a live-region fallback label', () => {
    for (const level of ['normal', 'info', 'success', 'warning', 'error', 'loading']) {
      expect(typeof getLocaleValue(enNotifications, `level.${level}`)).toBe('string');
      expect(typeof getLocaleValue(zhNotifications, `level.${level}`)).toBe('string');
    }
  });
});
