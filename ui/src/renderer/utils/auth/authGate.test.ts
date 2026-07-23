

import { describe, expect, test } from 'bun:test';
import { requiresCloudAuthGate, resolvePostLocalAuthPath } from './authGate';

describe('authGate', () => {
  test('WebUI does not require cloud auth before the product', () => {
    const previous = (globalThis as { window?: unknown }).window;
    try {
      (globalThis as { window?: { __backendPort?: number } }).window = {};
      expect(requiresCloudAuthGate()).toBe(false);
      expect(resolvePostLocalAuthPath(false)).toBe('/guid');
      expect(resolvePostLocalAuthPath(true)).toBe('/guid');
    } finally {
      (globalThis as { window?: unknown }).window = previous;
    }
  });

  test('desktop shell requires cloud auth before the product', () => {
    const previous = (globalThis as { window?: unknown }).window;
    try {
      (globalThis as { window?: { __backendPort?: number } }).window = { __backendPort: 4173 };
      expect(requiresCloudAuthGate()).toBe(true);
      expect(resolvePostLocalAuthPath(false)).toBe('/cloud-login');
      expect(resolvePostLocalAuthPath(true)).toBe('/guid');
    } finally {
      (globalThis as { window?: unknown }).window = previous;
    }
  });
});
