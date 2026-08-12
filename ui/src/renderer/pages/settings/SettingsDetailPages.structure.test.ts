import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relativePath: string) =>
  readFileSync(new URL(relativePath, import.meta.url), 'utf8');

describe('settings detail-page visual contracts', () => {
  test('uses the shared page contract for application settings without nested scroll owners', () => {
    const browser = read('../../components/settings/SettingsModal/contents/BrowserUseSettingsContent.tsx');
    const computer = read('../../components/settings/SettingsModal/contents/ComputerUseSettingsContent.tsx');

    expect(browser).toContain('SettingsPageHeader');
    expect(browser).toContain('SettingsGroup');
    expect(browser).not.toContain('NomiScrollArea');
    expect(computer).toContain('SettingsPermissionRow');
    expect(computer).not.toContain('NomiScrollArea');
  });

  test('keeps explicit-save drafts local to their pages', () => {
    const media = read('./MediaSettings.tsx');
    const poi = read('./PoiSettings.tsx');
    const insights = read('./InsightsSettings.tsx');
    const moa = read('./MoaSettings.tsx');
    const cloud = read('./CloudLoginSettings.tsx');

    for (const source of [media, poi, insights, moa, cloud]) {
      expect(source).toContain('SettingsActionBar');
    }
    expect(media).toContain('savedSettings');
    expect(poi).toContain('savedSettings');
    expect(insights).toContain('savedDraft');
    expect(moa).toContain('savedForm');
    expect(cloud).toContain('savedServerSettings');
  });

  test('shares text tabs and a single scroll owner across capability hubs', () => {
    const skills = read('./SkillsSettingsPage.tsx');
    const presets = read('./PresetSettings/index.tsx');
    const mcp = read('../mcp/index.tsx');
    const tools = read('../../components/settings/SettingsModal/contents/ToolsModalContent.tsx');

    expect(skills).toContain("layout='hub'");
    for (const source of [skills, presets, mcp]) {
      expect(source).toContain('flowy-settings-tabs');
    }
    expect(presets).not.toContain('NomiScrollArea');
    expect(tools).not.toContain('NomiScrollArea');
  });

  test('uses responsive control layouts and stable dependent sections on the edited form pages', () => {
    const browser = read('../../components/settings/SettingsModal/contents/BrowserUseSettingsContent.tsx');
    const computer = read('../../components/settings/SettingsModal/contents/ComputerUseSettingsContent.tsx');
    const poi = read('./PoiSettings.tsx');
    const learning = read('./LearningSettings.tsx');
    const insights = read('./InsightsSettings.tsx');
    const moa = read('./MoaSettings.tsx');

    expect(browser).toContain("controlLayout='compound'");
    expect(computer).toContain('SettingsRow');
    expect(computer).not.toContain("defaultValue: 'Yes'");
    expect(poi).toContain("sectionAutomatic");
    expect(poi).toContain('disabled={!settings.autoExtractEnabled}');
    expect(learning).toContain('SettingsControlGroup');
    expect(insights).toContain("t('common.yes')");
    expect(moa).toContain('SettingsEmptyState');
    expect(moa).toContain("controlLayout='compound'");
  });
});
