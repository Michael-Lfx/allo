/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test, beforeEach } from 'bun:test';
import {
  LOW_CREDITS_THRESHOLD,
  resolveCreditsBubbleState,
  getTodayDismissKey,
  isCreditsBubbleDismissed,
  dismissCreditsBubble,
  resetCreditsBubbleDismiss,
} from './creditsBubbleModel';

const createStorageMock = () => {
  let store: Record<string, string> = {};
  return {
    getItem: (key: string) => store[key] ?? null,
    setItem: (key: string, val: string) => {
      store[key] = String(val);
    },
    removeItem: (key: string) => {
      delete store[key];
    },
    clear: () => {
      store = {};
    },
    key: (index: number) => Object.keys(store)[index] ?? null,
    get length() {
      return Object.keys(store).length;
    },
  };
};

const mockStorage = createStorageMock();

// Set mock on globalThis
(globalThis as unknown as { window: { localStorage: typeof mockStorage } }).window = {
  localStorage: mockStorage,
};

describe('creditsBubbleModel', () => {
  beforeEach(() => {
    mockStorage.clear();
  });

  describe('resolveCreditsBubbleState', () => {
    test('returns normal when unauthenticated regardless of balance', () => {
      expect(
        resolveCreditsBubbleState({
          balance: 0,
          authenticated: false,
        })
      ).toBe('normal');

      expect(
        resolveCreditsBubbleState({
          balance: 500,
          authenticated: false,
        })
      ).toBe('normal');
    });

    test('returns normal when still fetching initial balance with zero refresh timestamp', () => {
      expect(
        resolveCreditsBubbleState({
          balance: 0,
          authenticated: true,
          isFetchingBalance: true,
          lastRefreshAt: 0,
        })
      ).toBe('normal');
    });

    test('returns exhausted when authenticated and balance is 0 or negative', () => {
      expect(
        resolveCreditsBubbleState({
          balance: 0,
          authenticated: true,
          lastRefreshAt: 1000,
        })
      ).toBe('exhausted');

      expect(
        resolveCreditsBubbleState({
          balance: -50,
          authenticated: true,
          lastRefreshAt: 1000,
        })
      ).toBe('exhausted');
    });

    test('returns low when authenticated and 0 < balance < 1000', () => {
      expect(
        resolveCreditsBubbleState({
          balance: 1,
          authenticated: true,
          lastRefreshAt: 1000,
        })
      ).toBe('low');

      expect(
        resolveCreditsBubbleState({
          balance: 999,
          authenticated: true,
          lastRefreshAt: 1000,
        })
      ).toBe('low');
    });

    test('returns normal when balance >= 1000', () => {
      expect(
        resolveCreditsBubbleState({
          balance: LOW_CREDITS_THRESHOLD,
          authenticated: true,
          lastRefreshAt: 1000,
        })
      ).toBe('normal');

      expect(
        resolveCreditsBubbleState({
          balance: 50000,
          authenticated: true,
          lastRefreshAt: 1000,
        })
      ).toBe('normal');
    });
  });

  describe('dismiss tracking and anti-fatigue', () => {
    test('formats today key as YYYY-MM-DD', () => {
      const fixedDate = new Date(2026, 8, 24); // Sept 24, 2026
      expect(getTodayDismissKey(fixedDate)).toBe('2026-09-24');
    });

    test('tracks dismiss per user, location, and state', () => {
      expect(isCreditsBubbleDismissed('user_1', 'sider', 'low')).toBe(false);

      dismissCreditsBubble('user_1', 'sider', 'low');
      expect(isCreditsBubbleDismissed('user_1', 'sider', 'low')).toBe(true);

      // Other location or state is not affected
      expect(isCreditsBubbleDismissed('user_1', 'sendbox', 'low')).toBe(false);
      expect(isCreditsBubbleDismissed('user_1', 'sider', 'exhausted')).toBe(false);
      expect(isCreditsBubbleDismissed('user_2', 'sider', 'low')).toBe(false);
    });

    test('resets all dismiss records for a given user', () => {
      dismissCreditsBubble('user_1', 'sider', 'low');
      dismissCreditsBubble('user_1', 'sendbox', 'exhausted');
      dismissCreditsBubble('user_2', 'sider', 'low');

      expect(isCreditsBubbleDismissed('user_1', 'sider', 'low')).toBe(true);
      expect(isCreditsBubbleDismissed('user_1', 'sendbox', 'exhausted')).toBe(true);

      resetCreditsBubbleDismiss('user_1');

      expect(isCreditsBubbleDismissed('user_1', 'sider', 'low')).toBe(false);
      expect(isCreditsBubbleDismissed('user_1', 'sendbox', 'exhausted')).toBe(false);
      // user_2 remains dismissed
      expect(isCreditsBubbleDismissed('user_2', 'sider', 'low')).toBe(true);
    });
  });
});
