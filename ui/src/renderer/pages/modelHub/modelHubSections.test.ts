/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import zhSettings from '@/renderer/services/i18n/locales/zh-CN/settings.json';
import enSettings from '@/renderer/services/i18n/locales/en-US/settings.json';

const src = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');

/** The sidebar order, top to bottom, across all three groups. */
const SECTIONS = [
  'models',
  'chat',
  'asr',
  'tts',
  'vision',
  'image',
  'video',
  'embedding',
  'free',
  'creation',
  'global',
] as const;

const GROUPS = [
  { key: 'access', sections: ['models'] },
  { key: 'capability', sections: ['chat', 'asr', 'tts', 'vision', 'image', 'video', 'embedding'] },
  { key: 'advanced', sections: ['free', 'creation', 'global'] },
] as const;

const hubOf = (locale: unknown): Record<string, string> =>
  (locale as { modelHub: Record<string, string> }).modelHub;

describe('model hub is a capability-first view', () => {
  test('the eleven sections exist in the designed order', () => {
    const start = src.indexOf('const SECTION_KEYS');
    const list = src.slice(start, src.indexOf('];', start));
    const keys = [...list.matchAll(/'([a-z]+)'/g)].map((m) => m[1]);
    expect(keys).toEqual([...SECTIONS]);
  });

  test('providers lead the sidebar — a provider is the source of every model', () => {
    expect(SECTIONS[0]).toBe('models');
    const groupStart = src.indexOf('const SECTION_GROUPS');
    const groupSrc = src.slice(groupStart, src.indexOf('const FLAT_SECTIONS', groupStart));
    // Group order, and the section order inside each group, both matter.
    const groupKeys = [...groupSrc.matchAll(/^ {4}key: '([a-z]+)',$/gm)].map((m) => m[1]);
    expect(groupKeys).toEqual(GROUPS.map((g) => g.key));
    const sectionKeys = [...groupSrc.matchAll(/^ {8}key: '([a-z]+)',$/gm)].map((m) => m[1]);
    expect(sectionKeys).toEqual([...SECTIONS]);
  });

  test('the free-model section sits in the last group, below the capabilities', () => {
    // Same rule the provider groups inside every capability section follow:
    // NomiFun-managed models rank below what the user configured.
    const advanced = GROUPS[GROUPS.length - 1].sections as readonly string[];
    expect(advanced.includes('free')).toBe(true);
    expect(SECTIONS.indexOf('free')).toBeGreaterThan(SECTIONS.indexOf('embedding'));
  });

  test('the default section is 对话, not the provider list', () => {
    expect(src.includes("resolveSection(searchParams.get('section')) ?? 'chat'")).toBe(true);
  });

  test('old bookmarks keep working', () => {
    // `speech` was a HOST holding the whole voice story; it now has one section
    // per direction, so an old link resolves to the first of them.
    expect(src.includes("speech: 'asr'")).toBe(true);
    // `creation` / `global` are still real keys — the capability sections did not
    // replace the cross-capability views, so those links resolve as-is.
    for (const stillReal of ['models', 'free', 'creation', 'global'] as const) {
      expect(SECTIONS.includes(stillReal)).toBe(true);
    }
    expect(src.includes("searchParams.get('section') === 'agents'")).toBe(true);
  });

  test('the cross-capability views survive the capability split', () => {
    // 创作能力 is the image ∪ video OVERVIEW, and 全局模型设置 owns the IDMM
    // defaults, the failover queue, and decision activity. Neither is a
    // per-capability concern, so neither is replaced by the new sections.
    expect(src.includes('<CreationModelsContent />')).toBe(true);
    expect(src.includes('<GlobalModelConfig />')).toBe(true);
  });

  test('every section and group has a label in both locales', () => {
    const labelKey = (s: string) => `section${s[0].toUpperCase()}${s.slice(1)}`;
    const groupKey = (g: string) => `group${g[0].toUpperCase()}${g.slice(1)}`;
    for (const locale of [zhSettings, enSettings]) {
      const hub = hubOf(locale);
      for (const key of [...SECTIONS.map(labelKey), ...GROUPS.map((g) => groupKey(g.key))]) {
        expect(typeof hub[key]).toBe('string');
        expect(hub[key].trim().length > 0).toBe(true);
      }
    }
  });

  test('the retired host label is gone from both locales', () => {
    for (const locale of [zhSettings, enSettings]) {
      expect(hubOf(locale).sectionSpeech).toBeUndefined();
    }
  });

  test('the group captions stay out of the a11y tree', () => {
    // `tablist` may own only `tab` children. The captions are decoration; the
    // tabs already carry their own labels and position.
    expect(src.includes("aria-hidden='true'")).toBe(true);
    expect(src.includes("role='tablist'")).toBe(true);
  });
});
