import { describe, expect, test } from 'bun:test';
import {
  filterSkillMarketItems,
  isSkillMarketItemInstalled,
  marketSkillInstallErrorMessage,
  normalizeSkillMarketErrors,
  normalizeSkillMarketItem,
  normalizeSkillMarketItems,
  resolveMarketSyncItems,
  SKILL_MARKET_SOURCES,
  translateMarketDescription,
} from './skillMarket';

const skillHubItem = {
  id: 'skillhub:owner/skills/demo',
  source: 'skillhub' as const,
  rank: 1,
  name: 'demo skill',
  description: 'GitHub coding helper',
  url: 'https://skillhub.cn/skills/owner/demo',
  install_mode: 'native' as const,
  install_command: '',
  tags: ['developer', 'coding'],
  audience_tags: ['developer'],
  scenario_tags: ['coding'],
};

const pluginItem = {
  ...skillHubItem,
  id: 'clawhub_plugins:openclaw/whatsapp',
  source: 'clawhub_plugins' as const,
  url: 'https://clawhub.ai/openclaw/plugins/whatsapp',
  install_mode: 'external' as const,
  install_command: 'openclaw plugins install clawhub:@openclaw/whatsapp',
};

describe('skill market helpers', () => {
  test('ordinary Skill market has one source and filters its structured identity', () => {
    expect(SKILL_MARKET_SOURCES).toEqual(['skillhub']);
    expect(
      filterSkillMarketItems([skillHubItem], 'skillhub', 'github', {
        audience: ['developer'],
        scenario: ['coding'],
      }),
    ).toEqual([skillHubItem]);
    expect(filterSkillMarketItems([skillHubItem], 'skillhub', 'missing', { audience: [], scenario: [] })).toHaveLength(0);
  });

  test('does not accept external CLI commands or untrusted URLs for ordinary SkillHub data', () => {
    expect(normalizeSkillMarketItem({ ...skillHubItem, install_command: 'external skill install owner/demo' })).toBeNull();
    expect(normalizeSkillMarketItem({ ...skillHubItem, url: 'https://example.com/owner/demo' })).toBeNull();
    expect(normalizeSkillMarketItem({ ...pluginItem, install_command: 'openclaw plugins install @x; rm -rf ~' })).toBeNull();
    expect(normalizeSkillMarketItems([pluginItem, { bad: true }])).toHaveLength(1);
    expect(normalizeSkillMarketErrors(['ok', 1, 'x'.repeat(400)])).toEqual(['ok', 'x'.repeat(240)]);
  });

  test('keeps independent plugin market normalization available', () => {
    expect(normalizeSkillMarketItem(pluginItem)?.source).toBe('clawhub_plugins');
    expect(normalizeSkillMarketItem({ ...pluginItem, artifact_url: 'https://evil.example/plugin.zip' })).toBeNull();
  });

  test('keeps cached market items only for the legacy independent sync helper', () => {
    expect(resolveMarketSyncItems([pluginItem], [])).toEqual([pluginItem]);
    expect(resolveMarketSyncItems([], [pluginItem])).toEqual([pluginItem]);
  });

  test('recognizes installed skills by canonical slug or display name', () => {
    expect(isSkillMarketItemInstalled(skillHubItem, ['demo'])).toBe(true);
    expect(isSkillMarketItemInstalled({ ...skillHubItem, name: 'Demo Skill' }, ['demo-skill'])).toBe(true);
    expect(isSkillMarketItemInstalled(skillHubItem, ['another-skill'])).toBe(false);
  });

  test('translates common market descriptions for zh display', () => {
    expect(translateMarketDescription('Ranked SkillHub skill from vercel-labs/skills.', skillHubItem)).toBe(
      '来自 vercel-labs/skills 的 SkillHub 榜单技能。',
    );
    expect(translateMarketDescription('GitHub coding helper', skillHubItem).includes('开发')).toBe(true);
  });

  test('maps native install error codes to dedicated i18n keys', () => {
    const cases: Array<[string, string]> = [
      ['MARKET_SKILL_SOURCE_UNSUPPORTED', 'settings.skillsMarket.installUnsupported'],
      ['MARKET_SKILL_ID_INVALID', 'settings.skillsMarket.installInvalidId'],
      ['MARKET_SKILL_NOT_FOUND', 'settings.skillsMarket.installNotFound'],
      ['MARKET_SKILL_NAME_CONFLICT', 'settings.skillsMarket.installConflict'],
      ['MARKET_SKILL_ARTIFACT_INVALID', 'settings.skillsMarket.installArtifactInvalid'],
      ['MARKET_SKILL_MANIFEST_INVALID', 'settings.skillsMarket.installManifestInvalid'],
      ['MARKET_SKILL_BUNDLE_UNSUPPORTED', 'settings.skillsMarket.installBundleUnsupported'],
      ['MARKET_SKILL_NETWORK', 'settings.skillsMarket.installNetwork'],
      ['MARKET_SKILL_TIMEOUT', 'settings.skillsMarket.installTimeout'],
      ['MARKET_SKILL_LOCAL_IO', 'settings.skillsMarket.installLocalIo'],
    ];
    for (const [code, key] of cases) {
      const mapped = marketSkillInstallErrorMessage(code);
      expect(mapped.key).toBe(key);
      expect(mapped.fallback.length).toBeGreaterThan(0);
    }
  });

  test('falls back to the generic install failure for unknown codes', () => {
    for (const code of ['', 'SOMETHING_ELSE', 'market_skill_network']) {
      expect(marketSkillInstallErrorMessage(code).key).toBe('settings.skillsMarket.installFailed');
    }
  });
});
