import { describe, expect, test } from 'bun:test';
import enNomi from './locales/en-US/nomi.json';
import zhNomi from './locales/zh-CN/nomi.json';
import enSettings from './locales/en-US/settings.json';
import zhSettings from './locales/zh-CN/settings.json';

describe('companion feedback locales', () => {
  for (const [name, nomi, settings] of [
    ['en-US', enNomi, enSettings],
    ['zh-CN', zhNomi, zhSettings],
  ] as const) {
    test(`${name} provides compact badges and their full tooltip labels`, () => {
      for (const level of ['l1', 'l2', 'l3', 'l4', 'l5'] as const) {
        expect(nomi.levels.badge[level].trim().length).toBeGreaterThan(0);
        expect(nomi.levels[level].trim().length).toBeGreaterThan(0);
      }
    });

    test(`${name} keeps the recreated workspace path in the warning`, () => {
      expect(settings.workDirRecreatedNotice).toContain('{{path}}');
    });
  }
});
