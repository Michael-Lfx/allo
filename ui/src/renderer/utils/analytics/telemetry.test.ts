import { describe, expect, test } from 'bun:test';
import { getInstallId, resetInstallIdForTests, adoptBackendClientId } from './identity';
import {
  resetTelemetryForTests,
  sanitizeProductProperties,
  isTelemetryOptedOut,
  setTelemetryOptOut,
  isTelemetryEnabled,
} from './telemetry';

describe('product telemetry allowlist', () => {
  test('drops prompt and path-like keys', () => {
    const sanitized = sanitizeProductProperties({
      feature: 'knowledge',
      hit_count: 3,
      prompt: 'secret question',
      root_path: 'C:\\Users\\admin\\docs',
    });
    expect(sanitized.feature).toBe('knowledge');
    expect(sanitized.hit_count).toBe(3);
    expect('prompt' in sanitized).toBe(false);
    expect('root_path' in sanitized).toBe(false);
  });

  test('opt-out disables telemetry without requiring SDK keys', () => {
    resetTelemetryForTests();
    expect(isTelemetryEnabled()).toBe(true);
    setTelemetryOptOut(true);
    expect(isTelemetryOptedOut()).toBe(true);
    expect(isTelemetryEnabled()).toBe(false);
    setTelemetryOptOut(false);
    expect(isTelemetryEnabled()).toBe(true);
  });
});

describe('install id', () => {
  test('adopts backend client id', () => {
    resetInstallIdForTests();
    const local = getInstallId();
    expect(local.length).toBeGreaterThan(7);
    expect(adoptBackendClientId('11111111-2222-4333-8444-555555555555')).toBe(
      '11111111-2222-4333-8444-555555555555'
    );
    expect(getInstallId()).toBe('11111111-2222-4333-8444-555555555555');
  });
});
