
import { describe, expect, test } from 'bun:test';
import {
  DEFAULT_LANGUAGE,
  SUPPORTED_LANGUAGES,
  firstPaintLanguage,
  injectedOsLocale,
  normalizeLanguageCode,
} from './i18n';

describe('i18n language support', () => {
  test('only exposes simplified Chinese and English as supported app languages', () => {
    expect(SUPPORTED_LANGUAGES).toEqual(['zh-CN', 'en-US']);
    expect(DEFAULT_LANGUAGE).toBe('en-US');
  });

  test('normalizes removed locales away from their old language codes', () => {
    expect(normalizeLanguageCode('zh-TW')).toBe('zh-CN');
    expect(normalizeLanguageCode('zh-Hans-CN')).toBe('zh-CN');
    expect(normalizeLanguageCode('zh_CN')).toBe('zh-CN');
    expect(normalizeLanguageCode('ja-JP')).toBe(DEFAULT_LANGUAGE);
    expect(normalizeLanguageCode('ko-KR')).toBe(DEFAULT_LANGUAGE);
    expect(normalizeLanguageCode('tr-TR')).toBe(DEFAULT_LANGUAGE);
    expect(normalizeLanguageCode('ru-RU')).toBe(DEFAULT_LANGUAGE);
    expect(normalizeLanguageCode('uk-UA')).toBe(DEFAULT_LANGUAGE);
    expect(normalizeLanguageCode('C')).toBe(DEFAULT_LANGUAGE);
    expect(normalizeLanguageCode('POSIX')).toBe(DEFAULT_LANGUAGE);
  });

  test('injected OS locale is the desktop window hint, not navigator.language', () => {
    const previous = (globalThis as { window?: Window }).window;
    const fakeWindow = { __osLocale: 'zh-Hans-CN' } as Window;
    (globalThis as { window?: Window }).window = fakeWindow;
    try {
      expect(injectedOsLocale()).toBe('zh-Hans-CN');
    } finally {
      (globalThis as { window?: Window }).window = previous;
    }
  });

  test('first paint prefers localStorage then injected OS locale then English', () => {
    const g = globalThis as { window?: Window & { __osLocale?: string } };
    if (!g.window) {
      g.window = {} as Window;
    }
    const previousLocale = g.window.__osLocale;
    const store: Record<string, string> = {};
    const fakeStorage = {
      getItem: (key: string) => store[key] ?? null,
      setItem: (key: string, value: string) => {
        store[key] = value;
      },
      removeItem: (key: string) => {
        delete store[key];
      },
    };
    const previousStorage = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');
    Object.defineProperty(globalThis, 'localStorage', {
      configurable: true,
      value: fakeStorage,
    });
    try {
      g.window.__osLocale = 'zh-CN';
      expect(firstPaintLanguage()).toBe('zh-CN');
      fakeStorage.setItem('i18nextLng', 'en-US');
      expect(firstPaintLanguage()).toBe('en-US');
    } finally {
      g.window.__osLocale = previousLocale;
      if (previousStorage) {
        Object.defineProperty(globalThis, 'localStorage', previousStorage);
      } else {
        Reflect.deleteProperty(globalThis, 'localStorage');
      }
    }
  });
});
