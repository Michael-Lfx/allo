

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('PresetSettings page shell', () => {
  test('consumes highlight search params without an activeTab dependency', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));

    expect(source.includes("searchParams.get('highlight')")).toBe(true);
    expect(source.includes('handleHighlightConsumed')).toBe(true);
    // Highlight consumption is driven by the URL alone; the library/market tab
    // state may never gate it or the highlight survives a tab switch.
    expect(source.includes('}, [searchParams, setSearchParams]);')).toBe(true);
    expect(/handleHighlightConsumed[\s\S]*?\}, \[[^\]]*activeTab/.test(source)).toBe(false);
    expect(source.includes('assistant-skills-hub-tabs')).toBe(false);
  });

  test('renders through HubPageShell without nested flex height chains', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));

    expect(source.includes('<HubPageShell')).toBe(true);
    expect(source.includes('lazyload')).toBe(false);
    expect(source.includes('flex-1 min-h-0')).toBe(true);
  });

  test('uses the shared page-level header', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));

    expect(source.includes('hideHeader')).toBe(false);
    expect(source.includes("title={t('settings.presetsHub.title'")).toBe(true);
  });
});
