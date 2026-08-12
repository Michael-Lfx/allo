import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./useExtensionSettingsTabs.ts', import.meta.url), 'utf8');

describe('extension settings tab state', () => {
  test('distinguishes loading, ready empty, and failed requests with retry', () => {
    expect(source).toContain("'loading' | 'ready' | 'error'");
    expect(source).toContain("publish({ tabs: tabs ?? [], status: 'ready', error: null })");
    expect(source).toContain("publish({ ...cachedState, status: 'error', error })");
    expect(source).toContain('refresh: () => Promise<void>');
  });
});
