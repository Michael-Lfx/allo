import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { openOfficialWebsiteCredits } from './openOfficialWebsiteCredits';
import type { OfficialWebsiteCreditsOpener } from './openOfficialWebsiteCredits';

const source = readFileSync(new URL('./openOfficialWebsiteCredits.ts', import.meta.url), 'utf8');

function opener(overrides: Partial<OfficialWebsiteCreditsOpener> = {}): OfficialWebsiteCreditsOpener & {
  entries: Array<{ language?: string; landing?: 'credits' }>;
  urls: string[];
  errors: string[];
} {
  const entries: Array<{ language?: string; landing?: 'credits' }> = [];
  const urls: string[] = [];
  const errors: string[] = [];
  return {
    entries,
    urls,
    errors,
    getWebsiteEntry: async (params) => {
      entries.push(params);
      return { url: 'https://www.flowyaipc.com/?tab=credits&token=jwt&language=zh#pricing' };
    },
    openExternalUrl: async (url) => {
      urls.push(url);
    },
    showError: (message) => {
      errors.push(message);
    },
    translate: (key) => key,
    ...overrides,
  };
}

describe('openOfficialWebsiteCredits', () => {
  test('asks website-entry for the credits landing and opens that URL', async () => {
    const next = opener();
    await openOfficialWebsiteCredits('zh-CN', next);
    expect(next.entries).toEqual([{ language: 'zh-CN', landing: 'credits' }]);
    expect(next.urls).toEqual([
      'https://www.flowyaipc.com/?tab=credits&token=jwt&language=zh#pricing',
    ]);
    expect(next.errors).toEqual([]);
  });

  test('ignores a second click while the first open is in flight', async () => {
    let release!: (value: { url: string }) => void;
    const firstEntry = new Promise<{ url: string }>((resolve) => {
      release = resolve;
    });
    const urls: string[] = [];
    const first = opener({
      getWebsiteEntry: () => firstEntry,
      openExternalUrl: async (url) => {
        urls.push(url);
      },
    });
    const second = opener({
      openExternalUrl: async (url) => {
        urls.push(`second:${url}`);
      },
    });

    const pending = openOfficialWebsiteCredits('en', first);
    await openOfficialWebsiteCredits('en', second);
    expect(second.entries).toEqual([]);

    release({ url: 'https://www.flowyaipc.com/?tab=credits#pricing' });
    await pending;
    expect(urls).toEqual(['https://www.flowyaipc.com/?tab=credits#pricing']);
  });

  test('toasts when website-entry fails', async () => {
    const next = opener({
      getWebsiteEntry: async () => {
        throw new Error('offline');
      },
    });
    await openOfficialWebsiteCredits('zh-CN', next);
    expect(next.urls).toEqual([]);
    expect(next.errors).toEqual(['billing.openFailed']);
  });

  test('toasts when website-entry returns an empty URL', async () => {
    const next = opener({
      getWebsiteEntry: async () => ({ url: '  ' }),
    });
    await openOfficialWebsiteCredits('zh-CN', next);
    expect(next.urls).toEqual([]);
    expect(next.errors).toEqual(['billing.openFailed']);
  });

  test('keeps the cloud JWT behind website-entry instead of reading tokens in the renderer', () => {
    expect(source.includes('getWebsiteEntry')).toBe(true);
    expect(source.includes("landing: 'credits'")).toBe(true);
    expect(source.includes('openExternalUrl')).toBe(true);
    expect(source.includes('tokens.json')).toBe(false);
    expect(source.includes('access_token')).toBe(false);
  });
});
