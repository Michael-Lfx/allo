import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('SkillsSettingsPage tabs', () => {
  test('syncs the active tab from the URL without snapping back during a click', () => {
    const source = readSource(new URL('./SkillsSettingsPage.tsx', import.meta.url));

    expect(source).toContain('setActiveTab(nextTab);');
    expect(source).toContain('}, [searchParams]);');
    expect(source).not.toContain('}, [activeTab, searchParams]);');
  });

  test('keeps the tab panes out of competing viewport-height layout chains', () => {
    const pageSource = readSource(new URL('./SkillsSettingsPage.tsx', import.meta.url));
    const marketSource = readSource(new URL('./SkillMarketSettings.tsx', import.meta.url));

    expect(pageSource).not.toContain('lazyload');
    expect(pageSource).not.toContain('flex flex-col flex-1 min-h-0');
    expect(pageSource).toContain('[&_.arco-tabs-header-nav]:!min-h-40px');
    expect(pageSource).toContain('[&_.arco-tabs-header-title]:!font-500');
    expect(marketSource).toContain("<div className='w-full pb-16px'>");
    expect(marketSource).not.toContain("<div className='flex flex-col h-full w-full'>");
  });
});
