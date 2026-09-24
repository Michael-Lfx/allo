/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export const LOW_CREDITS_THRESHOLD = 1000;

export type CreditsBubbleState = 'normal' | 'low' | 'exhausted';
export type CreditsBubbleLocation = 'sider' | 'sendbox';

export interface ResolveCreditsStateOptions {
  balance: number;
  authenticated: boolean;
  isFetchingBalance?: boolean;
  lastRefreshAt?: number;
}

/**
 * Resolves the credits warning level based on authentication and balance.
 * Guards against unauthenticated sessions and initial unloaded state.
 */
export function resolveCreditsBubbleState(options: ResolveCreditsStateOptions): CreditsBubbleState {
  const { balance, authenticated, isFetchingBalance = false, lastRefreshAt = 0 } = options;

  if (!authenticated) {
    return 'normal';
  }

  // If initial fetch is still in flight and we haven't received a balance yet, do not falsely warn.
  if (isFetchingBalance && lastRefreshAt === 0) {
    return 'normal';
  }

  if (balance <= 0) {
    return 'exhausted';
  }

  if (balance < LOW_CREDITS_THRESHOLD) {
    return 'low';
  }

  return 'normal';
}

/**
 * Returns today's date key in YYYY-MM-DD local format for day-level anti-fatigue.
 */
export function getTodayDismissKey(date: Date = new Date()): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

const buildStorageKey = (
  userId: string | undefined,
  location: CreditsBubbleLocation,
  state: 'low' | 'exhausted'
): string => {
  const uid = userId?.trim() || 'default_user';
  return `nomifun:credits-bubble:dismiss:${uid}:${location}:${state}`;
};

/**
 * Checks whether the credits bubble has already been dismissed for today.
 */
export function isCreditsBubbleDismissed(
  userId: string | undefined,
  location: CreditsBubbleLocation,
  state: 'low' | 'exhausted'
): boolean {
  if (typeof window === 'undefined' || !window.localStorage) {
    return false;
  }
  try {
    const key = buildStorageKey(userId, location, state);
    const stored = window.localStorage.getItem(key);
    return stored === getTodayDismissKey();
  } catch {
    return false;
  }
}

/**
 * Dismisses the credits bubble for today.
 */
export function dismissCreditsBubble(
  userId: string | undefined,
  location: CreditsBubbleLocation,
  state: 'low' | 'exhausted'
): void {
  if (typeof window === 'undefined' || !window.localStorage) {
    return;
  }
  try {
    const key = buildStorageKey(userId, location, state);
    window.localStorage.setItem(key, getTodayDismissKey());
  } catch {
    // Ignore storage write errors (e.g. private mode quota)
  }
}

/**
 * Resets all dismiss records for a given user (e.g. after a successful recharge).
 */
export function resetCreditsBubbleDismiss(userId: string | undefined): void {
  if (typeof window === 'undefined' || !window.localStorage) {
    return;
  }
  try {
    const uid = userId?.trim() || 'default_user';
    const prefix = `nomifun:credits-bubble:dismiss:${uid}:`;
    const keysToRemove: string[] = [];
    for (let i = 0; i < window.localStorage.length; i++) {
      const key = window.localStorage.key(i);
      if (key && key.startsWith(prefix)) {
        keysToRemove.push(key);
      }
    }
    for (const key of keysToRemove) {
      window.localStorage.removeItem(key);
    }
  } catch {
    // Ignore storage errors
  }
}
