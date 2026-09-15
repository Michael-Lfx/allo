/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { MODEL_SHOWCASE } from '@/renderer/utils/model/modelShowcase';
import enConversation from './locales/en-US/conversation.json';
import zhConversation from './locales/zh-CN/conversation.json';

type LocaleJson = Record<string, unknown>;

/** Resolve a dotted path inside the `conversation` namespace JSON. */
function valueAt(locale: LocaleJson, path: string): unknown {
  let cursor: unknown = locale;
  for (const segment of path.split('.')) {
    if (
      !cursor ||
      typeof cursor !== 'object' ||
      !Object.prototype.hasOwnProperty.call(cursor, segment)
    ) {
      return undefined;
    }
    cursor = (cursor as Record<string, unknown>)[segment];
  }
  return cursor;
}

const registeredTaglineKeys = [
  ...new Set(
    Object.values(MODEL_SHOWCASE)
      .map((entry) => entry.taglineKey)
      .filter((key): key is NonNullable<typeof key> => Boolean(key))
      .map((key) => key.replace(/^conversation\./, ''))
  ),
].sort();

describe('model picker tagline locales', () => {
  test('registers taglines for the showcased catalog models', () => {
    expect(registeredTaglineKeys.length).toBeGreaterThan(0);
    expect(registeredTaglineKeys).toContain('modelPicker.tagline.deepseek-v4-1-flash');
  });

  test('every registered tagline resolves in both locales', () => {
    const locales = [
      ['en-US', enConversation as LocaleJson],
      ['zh-CN', zhConversation as LocaleJson],
    ] as const;
    const unresolved = registeredTaglineKeys.flatMap((key) =>
      locales
        .filter(([, locale]) => {
          const value = valueAt(locale, key);
          return typeof value !== 'string' || value.trim().length === 0;
        })
        .map(([localeName]) => `${localeName}:${key}`)
    );

    expect(unresolved).toEqual([]);
  });

  test('both locales expose the same tagline keys and badge label', () => {
    const enTaglines = valueAt(enConversation as LocaleJson, 'modelPicker.tagline') as Record<string, string>;
    const zhTaglines = valueAt(zhConversation as LocaleJson, 'modelPicker.tagline') as Record<string, string>;

    expect(Object.keys(enTaglines).sort()).toEqual(Object.keys(zhTaglines).sort());
    expect(typeof valueAt(enConversation as LocaleJson, 'modelPicker.recommended')).toBe('string');
    expect(typeof valueAt(zhConversation as LocaleJson, 'modelPicker.recommended')).toBe('string');
  });
});
