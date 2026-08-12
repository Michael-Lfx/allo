import { describe, expect, test } from 'bun:test';
import { createMarketItemViewModel } from './marketViewModel';
import type { ISkillMarketItem } from '@/common/adapter/ipcBridge';

const item: ISkillMarketItem = {
  id: 'skill-1',
  source: 'skillhub',
  rank: 4,
  name: 'Example skill',
  description: 'A useful skill.',
  url: 'https://skillhub.cn/skills/example',
  install_command: 'npx skills add example',
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
});
