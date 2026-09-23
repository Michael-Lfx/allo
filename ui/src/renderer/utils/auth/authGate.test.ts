

import { describe, expect, test } from 'bun:test';
import { requiresCloudAuthGate, resolvePostLocalAuthPath } from './authGate';

describe('authGate', () => {
  test('requires cloud auth on both WebUI and desktop shell', () => {
    expect(requiresCloudAuthGate()).toBe(true);
    expect(resolvePostLocalAuthPath(false)).toBe('/cloud-login');
    expect(resolvePostLocalAuthPath(true)).toBe('/guid');
  });
});
