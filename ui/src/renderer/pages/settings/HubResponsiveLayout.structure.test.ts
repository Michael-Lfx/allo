import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relativePath: string) => readFileSync(new URL(relativePath, import.meta.url), 'utf8');

describe('settings capability hubs responsive layout', () => {
  test('uses wider responsive card columns and avoids a redundant hub background surface', () => {
    const skills = read('./SkillsHubSettings.tsx');
    const presets = read('./PresetSettings/PresetListPanel.tsx');
    const market = read('./MarketSettingsPanel.tsx');

    for (const source of [skills, presets, market]) {
      expect(source).toContain("minmax(min(270px, 100%), 1fr)");
    }
    expect(skills).not.toContain("bg-fill-2 rounded-24px");
    expect(market).not.toContain("bg-fill-2 rounded-24px");
  });

  test('keeps compact source selection and import actions intact at narrow widths', () => {
    const skills = read('./SkillsHubSettings.tsx');
    const market = read('./MarketSettingsPanel.tsx');
    const toolbar = read('./skill/MarketToolbar.tsx');

    expect(skills).toContain('<Dropdown');
    expect(skills).toContain('!whitespace-nowrap');
    expect(toolbar).toContain('compactSourcePicker');
    expect(toolbar).toContain('new ResizeObserver');
    expect(toolbar).toContain('<Select');
  });

  test('caps card metadata without making long identifiers create taller cards', () => {
    const skillCard = read('./skill/SkillCard.tsx');
    const marketCard = read('./skill/SkillMarketCard.tsx');
    const presetCard = read('./PresetSettings/PresetCard.tsx');

    for (const source of [skillCard, presetCard]) {
      expect(source).toContain('const MAX_VISIBLE_TAGS = 3');
      expect(source).toContain('max-w-[156px]');
    }
    expect(marketCard).toContain('const MAX_VISIBLE_TAGS = 2');
    expect(marketCard).toContain('max-w-[156px]');
    expect(marketCard).toContain('!whitespace-nowrap');
  });
});
