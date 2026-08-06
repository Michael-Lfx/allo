import { describe, expect, test } from 'bun:test';
import { classifyCloudModelEnvironment } from './cloudModelEnvironment';

describe('classifyCloudModelEnvironment', () => {
  test('is ready after a successful authoritative catalog restore', () => {
    expect(
      classifyCloudModelEnvironment({
        resolvedModelCount: 2,
        cachedProviderModelCount: 2,
      })
    ).toEqual({ status: 'ready' });
  });

  test('is degraded when sync fails but the authoritative resolver still returns models', () => {
    const syncError = new Error('sync unavailable');

    expect(
      classifyCloudModelEnvironment({
        resolvedModelCount: 1,
        cachedProviderModelCount: 1,
        syncError,
      })
    ).toEqual({ status: 'degraded', error: syncError });
  });

  test('is failed when neither the restored nor cached catalog has models', () => {
    const resolveError = new Error('catalog unavailable');

    expect(
      classifyCloudModelEnvironment({
        resolvedModelCount: 0,
        cachedProviderModelCount: 0,
        resolveError,
      })
    ).toEqual({ status: 'failed', error: resolveError });
  });

  test('blocks the app when the resolver returns zero even if provider metadata is present', () => {
    const resolveError = new Error('resolver unavailable');

    expect(
      classifyCloudModelEnvironment({
        resolvedModelCount: 0,
        cachedProviderModelCount: 3,
        resolveError,
      })
    ).toEqual({ status: 'failed', error: resolveError });
  });
});
