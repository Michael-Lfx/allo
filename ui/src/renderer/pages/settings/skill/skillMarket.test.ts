import { describe, expect, test } from 'bun:test';
import {
  filterSkillMarketItems,
  managedInstallErrorMessage,
  normalizeSkillMarketErrors,
  normalizeSkillMarketItem,
  normalizeSkillMarketItems,
  resolveMarketSyncItems,
  selectMarketSourceWithItems,
  translateMarketDescription,
} from './skillMarket';

const item = {
  id: 'skillhub:owner/skills/demo',
  source: 'skillhub' as const,
  resource_kind: 'skill' as const,
  install_mode: 'managed' as const,
  rank: 1,
  name: 'demo skill',
  description: 'GitHub coding helper',
  url: 'https://skillhub.cn/skills/demo',
  tags: ['developer', 'coding'],
  audience_tags: ['developer'],
  scenario_tags: ['coding'],
};

describe('skill market helpers', () => {
  const packageItem = {
    ...item,
    id: 'skillhub_packages:tech-test-automation',
    source: 'skillhub_packages' as const,
    resource_kind: 'skill_package' as const,
    install_mode: 'manual' as const,
    url: 'https://skillhub.cn/skillspackage/tech-test-automation',
    install_command: 'skillhub package add tech-test-automation',
  };

  test('filters by source, search, and shared tags', () => {
    const result = filterSkillMarketItems([item], 'skillhub', 'github', {
      audience: ['developer'],
      scenario: ['coding'],
    });

    expect(result).toEqual([item]);
    expect(filterSkillMarketItems([item], 'skillhub', '', { audience: [], scenario: [] })).toEqual([item]);
    expect(filterSkillMarketItems([item], 'skillhub', 'missing', { audience: [], scenario: [] })).toHaveLength(0);
    expect(filterSkillMarketItems([item], 'skillhub', '开发', { audience: [], scenario: [] })).toEqual([item]);
  });

  test('accepts managed SkillHub items without commands and rejects unsafe sources', () => {
    expect(normalizeSkillMarketItem(item)).toEqual(item);
    expect(
      normalizeSkillMarketItem({
        ...item,
        install_command: 'npx skills add owner/demo',
      })
    ).toBeNull();
    expect(normalizeSkillMarketItem({ ...item, url: 'https://example.com/owner/demo' })).toBeNull();
    expect(normalizeSkillMarketItem({ ...item, url: 'https://skills.sh/owner/demo' })).toBeNull();
    expect(normalizeSkillMarketItem({ ...item, source: 'clawhub' })).toBeNull();
    expect(normalizeSkillMarketItems([item, { bad: true }])).toHaveLength(1);
    expect(normalizeSkillMarketErrors(['ok', 1, 'x'.repeat(400)])).toEqual(['ok', 'x'.repeat(240)]);
  });

  test('accepts supported external market sources only with safe add commands', () => {
    const loopHubItem = {
      ...item,
      id: 'loophub:12277',
      source: 'loophub' as const,
      resource_kind: 'skill' as const,
      install_mode: 'manual' as const,
      url: 'https://hub.cocoloop.cn/skills/12277',
      install_command: 'loophub skill download https://dl.cocoloop.cn/bss/skills/demo.zip',
    };
    const mcpItem = {
      ...item,
      id: 'skillhub_mcp:playwright',
      source: 'skillhub_mcp' as const,
      resource_kind: 'mcp' as const,
      install_mode: 'manual' as const,
      url: 'https://skillhub.cn/mcp/playwright',
      install_command: 'mcp market add skillhub:playwright',
    };
    const mcpWorldItem = {
      ...item,
      id: 'mcpworld:c7897f8abf0350fbbf5a7fccc3e79bb8',
      source: 'mcpworld' as const,
      resource_kind: 'mcp' as const,
      install_mode: 'manual' as const,
      url: 'https://www.mcpworld.com/zh/detail/c7897f8abf0350fbbf5a7fccc3e79bb8',
      install_command: 'mcp market add mcpworld:c7897f8abf0350fbbf5a7fccc3e79bb8',
    };
    const pluginItem = {
      ...item,
      id: 'clawhub_plugins:openclaw/whatsapp',
      source: 'clawhub_plugins' as const,
      resource_kind: 'plugin' as const,
      install_mode: 'manual' as const,
      url: 'https://clawhub.ai/openclaw/plugins/whatsapp',
      install_command: 'openclaw plugins install clawhub:@openclaw/whatsapp',
    };
    expect(normalizeSkillMarketItems([loopHubItem, mcpItem, mcpWorldItem, pluginItem, packageItem])).toHaveLength(5);
    expect(normalizeSkillMarketItem({ ...pluginItem, install_command: 'openclaw plugins install @x; rm -rf ~' })).toBeNull();
    expect(normalizeSkillMarketItem({ ...mcpWorldItem, url: 'https://evil.example/zh/detail/demo' })).toBeNull();
  });

  test('keeps SkillHub CDN package avatars and drops unsafe ones without rejecting the item', () => {
    const packageItem = {
      ...item,
      id: 'skillhub_packages:tech-test-automation',
      source: 'skillhub_packages' as const,
      resource_kind: 'skill_package' as const,
      install_mode: 'manual' as const,
      url: 'https://skillhub.cn/skillspackage/tech-test-automation',
      install_command: 'skillhub package add tech-test-automation',
    };
    const avatar =
      'https://cloudcache.tencent-cloud.com/qcloud/tea/app/skillhub/assets/source/ai-buddy-decouple/expert-profiles/tech-test-automation.v20260625.avif';

    expect(normalizeSkillMarketItem({ ...packageItem, avatar })?.avatar).toBe(avatar);
    expect(normalizeSkillMarketItem({ ...packageItem, avatar: 'javascript:alert(1)' })?.avatar).toBeUndefined();
    expect(normalizeSkillMarketItem({ ...packageItem, avatar: 'https://evil.example/x.avif' })?.avatar).toBeUndefined();
    expect(normalizeSkillMarketItem({ ...packageItem, avatar: 'https://cloudcache.tencent-cloud.com/x.svg' })?.avatar).toBeUndefined();
  });

  test('keeps cached market items when a sync returns no valid entries', () => {
    expect(resolveMarketSyncItems([item], [])).toEqual([item]);
    expect(resolveMarketSyncItems([], [item])).toEqual([item]);
  });

  test('selects the first configured source that has items when the active source is empty', () => {
    const loopHubItem = {
      ...item,
      id: 'loophub:12277',
      source: 'loophub' as const,
      resource_kind: 'skill' as const,
      install_mode: 'manual' as const,
      url: 'https://hub.cocoloop.cn/skills/12277',
      install_command: 'loophub skill download https://dl.cocoloop.cn/bss/skills/demo.zip',
    };

    expect(selectMarketSourceWithItems('skillhub', ['skillhub', 'loophub'], [loopHubItem])).toBe('loophub');
    expect(selectMarketSourceWithItems('loophub', ['skillhub', 'loophub'], [loopHubItem])).toBe('loophub');
    expect(selectMarketSourceWithItems('skillhub', ['skillhub'], [])).toBe('skillhub');
  });

  test('translates common market descriptions for zh display', () => {
    expect(translateMarketDescription('Ranked SkillHub skill from vercel-labs/skills.', item)).toBe(
      '来自 vercel-labs/skills 的 SkillHub 榜单技能。'
    );
    expect(translateMarketDescription('GitHub coding helper', item).includes('开发')).toBe(true);
  });

  test('maps managed install error codes to i18n keys with fallbacks', () => {
    expect(managedInstallErrorMessage('MARKET_SKILL_BUNDLE_UNSUPPORTED')).toEqual({
      key: 'settings.skillsMarket.installBundleUnsupported',
      fallback: '该条目是技能合集，暂不支持单技能安装。',
    });
    expect(managedInstallErrorMessage('MARKET_SKILL_NOT_FOUND').key).toBe(
      'settings.skillsMarket.installNotFound'
    );
    expect(managedInstallErrorMessage('SOMETHING_ELSE').key).toBe('settings.skillsMarket.installError');
  });
});
