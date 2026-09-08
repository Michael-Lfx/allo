import { describe, expect, test } from 'bun:test';
import type { ISkillHubMarketItem } from '@/common/adapter/ipcBridge';
import {
  isSkillHubMarketItem,
  isSkillHubMarketResponse,
  mergeSkillHubMarketItems,
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
});
