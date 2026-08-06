import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = () => readFileSync(new URL('./CloudAuthContext.tsx', import.meta.url), 'utf8');

describe('cloud authentication model-environment contract', () => {
  test('keeps authentication and model restoration as separate gates', () => {
    const text = source();
    expect(text.includes("setModelStatus('restoring')")).toBe(true);
    expect(text.includes("setModelStatus('ready')")).toBe(false);
    expect(text.includes('restoreModelEnvironment')).toBe(true);
    expect(text.includes('modelProfile.resolve.invoke')).toBe(true);
    expect(text.includes("setModelStatus('failed')")).toBe(true);
  });

  test('clears catalog caches across unauthenticated transitions and exposes retry', () => {
    const text = source();
    expect(text.includes('clearAvailableModelsCache()')).toBe(true);
    expect(text.includes('retryModelEnvironment')).toBe(true);
    expect(text.includes('await refresh()')).toBe(true);
  });
});
