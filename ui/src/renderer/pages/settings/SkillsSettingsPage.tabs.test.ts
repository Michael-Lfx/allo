import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('SkillsSettingsPage hub chrome', () => {
  test('uses the shared capability hub instead of inner library/market tabs', () => {
    const source = readSource(new URL('./SkillsSettingsPage.tsx', import.meta.url));

    expect(source).toContain('<CapabilityHubShell');
    expect(source).toContain("hub='skills'");
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
