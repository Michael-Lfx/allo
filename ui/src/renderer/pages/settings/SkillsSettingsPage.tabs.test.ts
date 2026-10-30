import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('SkillsSettingsPage hub chrome', () => {
  test('uses the skill store page instead of the settings capability tabs', () => {
    const source = readSource(new URL('./SkillsSettingsPage.tsx', import.meta.url));

    expect(source).toContain('<HubPageShell');
    expect(source).toContain("data-testid='skill-store-toolbar'");
    expect(source).toContain("data-testid='capability-hub-installed'");
    expect(source).toContain('settings.capabilityHub.discover');
    expect(source).toContain("useCapabilityHubRoute('skills')");
    expect(source).toContain('settings.skillsMarket.title');
    expect(source).not.toContain('<CapabilityHubShell');
    expect(source).not.toContain('setActiveTab(nextTab);');
    expect(source).not.toContain('flowy-settings-tabs');
    expect(source).not.toContain('lazyload');
  });

  test('keeps the market pane on the shared content contract', () => {
    const marketSource = readSource(new URL('./SkillMarketSettings.tsx', import.meta.url));

    expect(marketSource).toContain("<div className='w-full pb-16px'>");
    expect(marketSource).not.toContain("<div className='flex flex-col h-full w-full'>");
  });
});
