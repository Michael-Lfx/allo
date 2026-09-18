import { describe, expect, test } from 'bun:test';
import { createMarketItemViewModel, createSkillHubMarketItemViewModel, formatSkillHubMarketCount } from './marketViewModel';
import type { ISkillHubMarketItem, ISkillMarketItem } from '@/common/adapter/ipcBridge';

const item: ISkillMarketItem = {
  id: 'skill-1',
  source: 'skillhub',
  rank: 4,
  name: 'Example skill',
  description: 'A useful skill.',
  url: 'https://skillhub.cn/skills/example',
  install_mode: 'native',
  install_command: '',
  tags: ['requires_api_key', 'long-technical-tag', 'another-tag'],
  audience_tags: ['developer'],
  scenario_tags: ['coding'],
  stats: '0 downloads · 0 stars',
};

const t = (key: string, options?: Record<string, unknown>) => {
  const count = options?.count ?? 0;
  return `${key}:${count}`;
};

describe('market item view model', () => {
  test('formats SkillHub counters with compact k units after one thousand', () => {
    expect(formatSkillHubMarketCount(999)).toBe('999');
    expect(formatSkillHubMarketCount(1000)).toBe('1k');
    expect(formatSkillHubMarketCount(1250)).toBe('1.3k');
    expect(formatSkillHubMarketCount(299518)).toBe('299.5k');
  });

  test('keeps full metadata for details while bounding card metadata', () => {
    const model = createMarketItemViewModel(item, {
      localeKey: 'en-US',
      tagByKey: new Map([
        ['developer', { label: 'Developer' }],
        ['coding', { label: 'Coding' }],
      ]),
      t,
    });

    expect(model.visibleTags).toEqual(['Developer', 'Coding']);
    expect(model.allTags).toContain('long-technical-tag');
    expect(model.overflowTagCount).toBe(2);
    expect(model.compactStats).toBeUndefined();
    expect(model.fullStats).toBe('settings.market.downloadsCount:0 · settings.market.starsCount:0');
    expect(model.requiresApi).toBe(true);
    expect(model.avatar).toBeUndefined();
  });

  test('forwards a sanitized package avatar to the card', () => {
    const avatar =
      'https://cloudcache.tencent-cloud.com/qcloud/tea/app/skillhub/assets/source/ai-buddy-decouple/expert-profiles/tech-test-automation.v20260625.avif';
    const model = createMarketItemViewModel({ ...item, avatar }, { localeKey: 'en-US', t });
    expect(model.avatar).toBe(avatar);
  });

  test('localizes known statistic units and preserves unknown formats', () => {
    const model = createMarketItemViewModel({ ...item, stats: '12 downloads · 3 stars · 9 custom' }, { localeKey: 'en-US', t });
    expect(model.compactStats).toContain('settings.market.downloadsCount:12');
    expect(model.compactStats).toContain('9 custom');
  });

  test('keeps localized technical tags available in the full detail view', () => {
    const model = createMarketItemViewModel(
      { ...item, audience_tags: [], scenario_tags: [], tags: ['developer'] },
      { localeKey: 'en-US', tagByKey: new Map([['developer', { label: 'Developer' }]]), t },
    );
    expect(model.allTags).toEqual(['Developer']);
  });

  test('preserves structured SkillHub metadata and API-key unknown state', () => {
    const skillHubItem: ISkillHubMarketItem = {
      id: 'skillhub:owner/skills/example',
      owner: 'owner',
      slug: 'example',
      market_source: 'clawhub',
      upstream_source: 'clawhub',
      rank: 1,
      name: 'Example skill',
      description: 'A '.repeat(300),
      version: '1.2.3',
      category: 'development',
      tags: ['coding'],
      sub_categories: [{ key: 'testing', name: 'Testing' }],
      requires_api_key: null,
      downloads: 12,
      installs: 8,
      stars: 4,
      score: 9.5,
      created_at: 1_700_000_000_000,
      updated_at: 1_700_000_100_000,
      url: 'https://skillhub.cn/skills/owner/example',
      avatar: null,
    };
    const model = createSkillHubMarketItemViewModel(skillHubItem, { localeKey: 'en-US', t });

    expect(model.skillHub?.version).toBe('1.2.3');
    expect(model.skillHub?.subCategories).toEqual([{ key: 'testing', name: 'Testing' }]);
    expect(model.skillHub?.requiresApiKey).toBeNull();
    expect(model.apiKeyUnknown).toBe(true);
    expect(model.fullDescription.length).toBeGreaterThan(220);
    expect(model.installCommand).toBe('');
  });
});
