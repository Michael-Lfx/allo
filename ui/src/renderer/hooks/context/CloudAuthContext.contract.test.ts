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
    expect(text.includes("Promise<CloudAuthRefreshResult>")).toBe(true);
    expect(text.includes("'authenticated' : 'stale'")).toBe(true);
    expect(text.includes("return 'offline'")).toBe(true);
    expect(text.includes("return 'stale'")).toBe(true);
  });

  test('waits for model restore by default and lets callers opt out', () => {
    const text = source();
    expect(text.includes('waitForModels?: boolean')).toBe(true);
    expect(text.includes('options.waitForModels !== false')).toBe(true);
    // Default path stays serialized behind a fully restored catalog...
    expect(
      text.includes('await restoreModelEnvironmentForRun(runId, controller, accountId, forceModelSync)')
    ).toBe(true);
    // ...while the login fast path fires the restore without awaiting it.
    expect(
      text.includes('void restoreModelEnvironmentForRun(runId, controller, accountId, forceModelSync)')
    ).toBe(true);
  });

  test('clears catalog caches across unauthenticated transitions and exposes retry', () => {
    const text = source();
    expect(text.includes('clearAvailableModelsCache()')).toBe(true);
    expect(text.includes('retryModelEnvironment')).toBe(true);
    expect(text.includes('await refresh({ forceModelSync: true })')).toBe(true);
  });

  test('listens for a global cloud-auth-expired event and shows a re-login modal', () => {
    const text = source();
    expect(text.includes('CLOUD_AUTH_EXPIRED_EVENT')).toBe(true);
    expect(text.includes('CloudSessionExpiredModal')).toBe(true);
    expect(text.includes("window.location.hash = '/cloud-login'")).toBe(true);
  });
});
