import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('PresetSettings page shell', () => {
  test('consumes highlight search params without an activeTab dependency', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));

    expect(source.includes("searchParams.get('highlight')")).toBe(true);
    expect(source.includes('handleHighlightConsumed')).toBe(true);
    expect(source.includes('}, [searchParams, setSearchParams]);')).toBe(true);
    expect(/handleHighlightConsumed[\s\S]*?\}, \[[^\]]*activeTab/.test(source)).toBe(false);
    expect(source.includes('assistant-skills-hub-tabs')).toBe(false);
  });

  test('renders through the shared capability hub chrome', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));

    expect(source.includes('<CapabilityHubShell')).toBe(true);
    expect(source.includes("hub='presets'")).toBe(true);
    expect(source.includes('marketEnabled={false}')).toBe(true);
    expect(source.includes('<PresetListPanel')).toBe(true);
    expect(source.includes('PresetPackageMarketSettings')).toBe(false);
    expect(source.includes('flowy-settings-tabs')).toBe(false);
  });

  test('hides the old page-level HubPageShell title', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));

    expect(source.includes('<HubPageShell')).toBe(false);
    expect(source.includes("title={t('settings.presetsHub.title'")).toBe(false);
  });

  test('uses the theme-aware primary action for creating a preset', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));
    const styles = readSource(new URL('../components/settings.css', import.meta.url));

    expect(source).toContain("type='primary'");
    expect(source).toContain('capability-hub-action-btn--primary');
    expect(styles).toContain('.capability-hub-action-btn.arco-btn:not(.capability-hub-action-btn--primary)');
    expect(styles).toContain('.capability-hub-action-btn--primary.arco-btn');
    expect(styles).toContain('font-weight: 600 !important;');
  });
});
