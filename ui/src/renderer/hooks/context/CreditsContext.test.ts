/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import {
  getDayKeyStorageKey,
  getMsUntilNextMidnight,
  getTodayKey,
  loadDayKey,
  loadMockCredits,
  saveDayKey,
  saveMockCredits,
} from './CreditsContext';

class MemoryStorage implements Storage {
  private store = new Map<string, string>();

  get length(): number {
    return this.store.size;
  }

  clear(): void {
    this.store.clear();
  }

  getItem(key: string): string | null {
    return this.store.get(key) ?? null;
  }

  key(index: number): string | null {
    return Array.from(this.store.keys())[index] ?? null;
  }

  removeItem(key: string): void {
    this.store.delete(key);
  }

  setItem(key: string, value: string): void {
    this.store.set(key, String(value));
  }
}

describe('CreditsContext helper functions', () => {
  let memoryStorage: MemoryStorage;
  let hadLocalStorage = false;
  let originalLocalStorage: unknown;

  beforeEach(() => {
    hadLocalStorage = 'localStorage' in globalThis;
    originalLocalStorage = (globalThis as { localStorage?: unknown }).localStorage;
    memoryStorage = new MemoryStorage();
    (globalThis as unknown as { localStorage: Storage }).localStorage = memoryStorage;
    if (typeof window !== 'undefined') {
      (window as unknown as { localStorage: Storage }).localStorage = memoryStorage;
    }
  });

  afterEach(() => {
    // Restore the environment so the storage stand-in can't leak into other
    // test files if they ever share a process.
    if (hadLocalStorage) {
      (globalThis as unknown as { localStorage: unknown }).localStorage = originalLocalStorage;
    } else {
      delete (globalThis as Record<string, unknown>).localStorage;
    }
  });

  describe('getTodayKey', () => {
    test('calculates correct integer date key YYYYMMDD', () => {
      const date = new Date(2026, 8, 23, 14, 30, 0); // Month 8 is September
      expect(getTodayKey(date)).toBe(20260923);
    });

    test('handles single digit months and days with leading zeroes', () => {
      const date = new Date(2026, 0, 5, 0, 0, 0); // Jan 5
      expect(getTodayKey(date)).toBe(20260105);
    });

    test('defaults to current date if omitted', () => {
      const now = new Date();
      const expected = Number(
        `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, '0')}${String(
          now.getDate()
        ).padStart(2, '0')}`
      );
      expect(getTodayKey()).toBe(expected);
    });
  });

  describe('getMsUntilNextMidnight', () => {
    test('calculates correct remaining milliseconds until next midnight', () => {
      const base = new Date(2026, 8, 23, 23, 59, 50, 0); // 10s before midnight
      const ms = getMsUntilNextMidnight(base);
      // Next midnight is 2026-09-24 00:00:01 (11s later = 11000ms)
      expect(ms).toBe(11000);
    });

    test('always returns at least 1000ms', () => {
      const base = new Date(2026, 8, 23, 0, 0, 0, 0);
      const ms = getMsUntilNextMidnight(base);
      expect(ms).toBeGreaterThanOrEqual(1000);
    });
  });

  describe('dayKey storage and multi-account isolation', () => {
    test('generates account-scoped storage keys', () => {
      expect(getDayKeyStorageKey('user_123')).toBe('nomifun:credits:lastCheckInDayKey:user_123');
      expect(getDayKeyStorageKey('')).toBe('nomifun:credits:lastCheckInDayKey');
      expect(getDayKeyStorageKey(undefined)).toBe('nomifun:credits:lastCheckInDayKey');
    });

    test('isolates dayKey storage across different accounts', () => {
      saveDayKey(20260923, 'account_alice');
      expect(loadDayKey('account_alice')).toBe(20260923);

      saveDayKey(20260922, 'account_bob');
      expect(loadDayKey('account_alice')).toBe(20260923);
      expect(loadDayKey('account_bob')).toBe(20260922);
    });

    test('falls back to global day key when account key is absent', () => {
      memoryStorage.setItem('nomifun:credits:lastCheckInDayKey', '20260920');
      expect(loadDayKey('account_charlie')).toBe(20260920);
    });

    test('saving an account day key does not leak to other accounts', () => {
      // Regression: Alice checks in today; Bob (never checked in on this
      // device) must still read 0 so his own daily check-in is not skipped.
      saveDayKey(20260923, 'account_alice');
      expect(loadDayKey('account_bob')).toBe(0);
    });

    test('first account-scoped save retires the legacy global key', () => {
      memoryStorage.setItem('nomifun:credits:lastCheckInDayKey', '20260920');
      // Legacy migration fallback still applies before the first scoped save.
      expect(loadDayKey('account_alice')).toBe(20260920);

      saveDayKey(20260923, 'account_alice');
      expect(memoryStorage.getItem('nomifun:credits:lastCheckInDayKey')).toBeNull();
      // With the legacy key retired, other accounts no longer inherit the day.
      expect(loadDayKey('account_bob')).toBe(0);
      expect(loadDayKey('account_alice')).toBe(20260923);
    });

    test('saveDayKey without accountId still writes the global key', () => {
      saveDayKey(20260923);
      expect(loadDayKey()).toBe(20260923);
    });

    test('returns 0 when no day key exists in storage', () => {
      expect(loadDayKey('account_new')).toBe(0);
      expect(loadDayKey(undefined)).toBe(0);
    });
  });

  describe('mock credits storage for dev/testing', () => {
    test('returns null when no mock credits are stored', () => {
      expect(loadMockCredits()).toBeNull();
    });

    test('saves and loads mock credits correctly', () => {
      saveMockCredits(500, true);
      expect(loadMockCredits()).toEqual({ balance: 500, authenticated: true });

      saveMockCredits(0, true);
      expect(loadMockCredits()).toEqual({ balance: 0, authenticated: true });

      saveMockCredits(2000, true);
      expect(loadMockCredits()).toEqual({ balance: 2000, authenticated: true });
    });

    test('clears mock credits when balance is null', () => {
      saveMockCredits(500, true);
      expect(loadMockCredits()).toEqual({ balance: 500, authenticated: true });

      saveMockCredits(null);
      expect(loadMockCredits()).toBeNull();
    });
  });
});
