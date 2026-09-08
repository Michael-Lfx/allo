import { describe, expect, test } from 'bun:test';
import type { ISkillHubMarketItem } from '@/common/adapter/ipcBridge';
import {
  isSkillHubMarketItem,
  isSkillHubMarketResponse,
  mergeSkillHubMarketItems,
  readSkillHubMarketQueryCache,
  skillHubMarketCacheKey,
  defaultSkillHubMarketSource,
} from './useSkillHubMarket';

const item = (id: string): ISkillHubMarketItem => ({
  id,
  owner: 'owner',
  slug: id.split('/').at(-1) ?? 'skill',
  market_source: 'skillhub',
  upstream_source: 'community',
  rank: 1,
  name: id,
  description: '',
  version: '1.0.0',
  category: null,
  tags: [],
  sub_categories: [],
  requires_api_key: null,
  downloads: 0,
  installs: 0,
  stars: 0,
  score: 0,
  created_at: null,
  updated_at: null,
  url: `https://skillhub.cn/skills/owner/${id.split('/').at(-1) ?? 'skill'}`,
  avatar: null,
});

describe('SkillHub market data boundaries', () => {
  test('derives the first source from the resolved i18n language', () => {
    expect(defaultSkillHubMarketSource('zh-CN')).toBe('skillhub');
    expect(defaultSkillHubMarketSource('en-US')).toBe('clawhub');
    expect(defaultSkillHubMarketSource(undefined)).toBe('clawhub');
  });
  test('cache key isolates every query dimension and page', () => {
    const base = {
      keyword: 'pdf',
      category: 'content',
      requires_api_key: true,
      sort_by: 'downloads' as const,
      page_size: 20,
      source: 'skillhub' as const,
    };
    const key = skillHubMarketCacheKey(base, 1);
    expect(key.startsWith('nomifun.skillHubMarket.v8.query:')).toBe(true);
    expect(key).toContain('"keyword":"pdf"');
    expect(key).toContain('"category":"content"');
    expect(key).toContain('"requires_api_key":true');
    expect(key).toContain('"sort_by":"downloads"');
    expect(key).toContain('"source":"skillhub"');
    expect(key).not.toBe(skillHubMarketCacheKey({ ...base, sort_by: 'score' }, 1));
    expect(key).not.toBe(skillHubMarketCacheKey({ ...base, source: 'clawhub' }, 1));
    expect(key).not.toBe(skillHubMarketCacheKey(base, 2));
  });

  test('deduplicates canonical IDs across pages without dropping first-page order', () => {
    const first = item('skillhub:owner/skills/one');
    const duplicate = { ...first, name: 'duplicate' };
    const second = item('skillhub:owner/skills/two');
    expect(mergeSkillHubMarketItems([first], [duplicate, second])).toEqual([first, second]);
  });

  test('cache validation rejects non-canonical identities and unsafe avatars', () => {
    const valid = item('skillhub:owner/skills/one');
    expect(isSkillHubMarketItem(valid)).toBe(true);
    expect(isSkillHubMarketItem({ ...valid, url: 'https://evil.example/skill' })).toBe(false);
    expect(isSkillHubMarketItem({ ...valid, id: 'skillhub:other/skills/one' })).toBe(false);
    expect(isSkillHubMarketItem({ ...valid, avatar: 'https://evil.example/icon.png' })).toBe(false);
    expect(isSkillHubMarketResponse({
      fetched_at: 1,
      total: 1,
      page: 1,
      page_size: 20,
      items: [valid],
    })).toBe(true);
    expect(isSkillHubMarketResponse({
      fetched_at: 1,
      total: 1,
      page: 1,
      page_size: 20,
      items: [{ ...valid, url: 'https://evil.example/skill' }],
    })).toBe(false);
  });

  test('accepts the canonical enterprise identity end to end', () => {
    // The backend resolves `namespace.handle` as the public owner; the
    // frontend must accept that canonical form verbatim (id and URL agree
    // with the owner) so it can render and cache it.
    const enterprise: ISkillHubMarketItem = {
      ...item('skillhub:tencent-adm/skills/agently-mail'),
      owner: 'tencent-adm',
      slug: 'agently-mail',
      url: 'https://skillhub.cn/skills/tencent-adm/agently-mail',
    };
    expect(isSkillHubMarketItem(enterprise)).toBe(true);
    // A payload whose id/URL disagree with their owner/slug is rejected; the
    // frontend never rewrites owner strings itself.
    expect(isSkillHubMarketItem({
      ...enterprise,
      url: 'https://skillhub.cn/skills/u_d95b6787/agently-mail',
    })).toBe(false);
    expect(isSkillHubMarketItem({
      ...enterprise,
      id: 'skillhub:u_d95b6787/skills/agently-mail',
    })).toBe(false);
  });

  test('never reads legacy v7 cache entries but reads its own v8 entries', () => {
    const store = new Map<string, string>();
    const storage: Storage = {
      get length() {
        return store.size;
      },
      clear: () => store.clear(),
      getItem: (key) => (store.has(key) ? store.get(key)! : null),
      key: (index) => [...store.keys()][index] ?? null,
      removeItem: (key) => {
        store.delete(key);
      },
      setItem: (key, value) => {
        store.set(key, String(value));
      },
    };
    const originalWindow = globalThis.window;
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      writable: true,
      value: { localStorage: storage },
    });
    try {
      const query = {
        keyword: 'agently-mail',
        source: 'skillhub' as const,
        category: undefined,
        requires_api_key: undefined,
        sort_by: 'score' as const,
        page_size: 20,
      };
      const entry = {
        cached_at: 1,
        response: {
          fetched_at: 1,
          total: 1,
          page: 1,
          page_size: 20,
          items: [item('skillhub:owner/skills/one')],
        },
      };
      const v8Key = skillHubMarketCacheKey(query, 1);
      const legacyKey = v8Key.replace('.v8.', '.v7.');
      expect(legacyKey).not.toBe(v8Key);

      // A leftover entry from the v7 generation is invisible to this build,
      // even when its payload would still pass item validation.
      storage.setItem(legacyKey, JSON.stringify(entry));
      expect(readSkillHubMarketQueryCache(v8Key)).toBeNull();

      // The same payload written under the v8 key is a normal cache hit.
      storage.setItem(v8Key, JSON.stringify(entry));
      expect(readSkillHubMarketQueryCache(v8Key)?.response.items).toHaveLength(1);

      // A v8 entry carrying a non-canonical item is rejected on read.
      storage.setItem(
        v8Key,
        JSON.stringify({
          ...entry,
          response: {
            ...entry.response,
            items: [{ ...item('skillhub:owner/skills/one'), url: 'https://evil.example/x' }],
          },
        }),
      );
      expect(readSkillHubMarketQueryCache(v8Key)).toBeNull();
    } finally {
      Object.defineProperty(globalThis, 'window', {
        configurable: true,
        writable: true,
        value: originalWindow,
      });
    }
  });
});
