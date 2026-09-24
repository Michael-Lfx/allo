import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { ipcBridge } from '@/common';
import { getCurrentCronTimeZone } from '@renderer/pages/cron/cronUtils';
import { emitter } from '@/renderer/utils/emitter';
import { useCloudAuth } from './CloudAuthContext';

// --- Day key (local midnight, YYYYMMDD integer) ------------------------------
// Mirrors FlowyClaw's getTodayKey(): the client-side gate flips at the user's
// local midnight. The server independently resolves the real day boundary from
// the posted `timeZone`, so the authoritative day key comes back in the
// response and is what we persist.
export function getTodayKey(date: Date = new Date()): number {
  return Number(
    `${date.getFullYear()}${String(date.getMonth() + 1).padStart(2, '0')}${String(
      date.getDate()
    ).padStart(2, '0')}`
  );
}

export function getMsUntilNextMidnight(now: Date = new Date()): number {
  const nextMidnight = new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate() + 1,
    0,
    0,
    1,
    0
  );
  return Math.max(1000, nextMidnight.getTime() - now.getTime());
}

// --- Persistence for lastCheckInDayKey ---------------------------------------
// Deliberately scoped by account ID so switching accounts on the same device
// does not prevent other accounts from performing their daily check-in.
// The unscoped global key is LEGACY-ONLY: it exists so installs from before
// account-scoping migrate their stored day once. Never write it for a known
// account — a global value always belongs to "some other account" and the
// migration fallback in loadDayKey() would let it suppress that account's
// first check-in of the day.
const GLOBAL_DAYKEY_STORAGE = 'nomifun:credits:lastCheckInDayKey';

/**
 * Safely resolves the active `localStorage` instance across browser runtime
 * and headless/unit test environments where window may be undefined.
 */
function getStorage(): Storage | null {
  try {
    if (typeof window !== 'undefined' && window.localStorage) {
      return window.localStorage;
    }
    if (typeof globalThis !== 'undefined' && (globalThis as { localStorage?: Storage }).localStorage) {
      return (globalThis as { localStorage: Storage }).localStorage;
    }
    return null;
  } catch {
    return null;
  }
}

export function getDayKeyStorageKey(accountId?: string): string {
  return accountId
    ? `nomifun:credits:lastCheckInDayKey:${accountId}`
    : GLOBAL_DAYKEY_STORAGE;
}

export function loadDayKey(accountId?: string): number {
  try {
    const storage = getStorage();
    if (!storage) return 0;
    const key = getDayKeyStorageKey(accountId);
    const val = storage.getItem(key);
    if (val) {
      return Number(val) || 0;
    }
    // Fallback migration: if accountId has no dedicated key yet, try global key
    if (accountId) {
      const globalVal = storage.getItem(GLOBAL_DAYKEY_STORAGE);
      if (globalVal) {
        return Number(globalVal) || 0;
      }
    }
    return 0;
  } catch {
    return 0;
  }
}

export function saveDayKey(key: number, accountId?: string): void {
  try {
    const storage = getStorage();
    if (!storage) return;
    storage.setItem(getDayKeyStorageKey(accountId), String(key));
    if (accountId) {
      // Migration complete for this device: drop the legacy global key so the
      // loadDayKey() fallback can't leak this account's day to other accounts.
      storage.removeItem(GLOBAL_DAYKEY_STORAGE);
    }
  } catch {
    // ignore storage failures (private mode, quota, etc.)
  }
}

// --- Mock Credits Support for Testing/Dev ------------------------------------
const MOCK_CREDITS_STORAGE = 'nomifun:credits:mockBalance';
const MOCK_AUTH_STORAGE = 'nomifun:credits:mockAuth';

export function loadMockCredits(): { balance: number; authenticated: boolean } | null {
  try {
    const storage = getStorage();
    if (!storage) return null;
    const rawBalance = storage.getItem(MOCK_CREDITS_STORAGE);
    if (rawBalance === null || rawBalance === undefined) return null;
    const balance = Number(rawBalance);
    if (Number.isNaN(balance)) return null;
    const rawAuth = storage.getItem(MOCK_AUTH_STORAGE);
    const authenticated = rawAuth === null ? true : rawAuth === 'true';
    return { balance, authenticated };
  } catch {
    return null;
  }
}

export function saveMockCredits(balance: number | null, authenticated = true): void {
  try {
    const storage = getStorage();
    if (!storage) return;
    if (balance === null) {
      storage.removeItem(MOCK_CREDITS_STORAGE);
      storage.removeItem(MOCK_AUTH_STORAGE);
    } else {
      storage.setItem(MOCK_CREDITS_STORAGE, String(balance));
      storage.setItem(MOCK_AUTH_STORAGE, authenticated ? 'true' : 'false');
    }
  } catch {
    // ignore storage errors
  }
}

// --- Auto-refresh scene throttle ---------------------------------------------
// Shared across the runtime so multiple consumers can't fan out duplicate
// requests. `mount` and `midnight` have no throttle; `focus`/`online`/`polling`
// are rate-limited.
export type RefreshScene = 'mount' | 'focus' | 'polling' | 'online' | 'midnight';
const SCENE_INTERVAL_MS: Record<RefreshScene, number> = {
  mount: 0,
  midnight: 0,
  online: 5_000,
  focus: 15_000,
  polling: 10 * 60_000,
};
const POLLING_INTERVAL_MS = 10 * 60_000;
const MANUAL_COOLDOWN_MS = 5_000;

let globalLastTriggerByScene: Record<RefreshScene, number> = {
  mount: 0,
  midnight: 0,
  online: 0,
  focus: 0,
  polling: 0,
};

// --- Context value -----------------------------------------------------------
export interface CreditsContextValue {
  balance: number;
  authenticated: boolean;
  lastCheckInDayKey: number;
  isFetchingBalance: boolean;
  isCheckingIn: boolean;
  lastRefreshAt: number;
  cooldownSeconds: number;
  canRefresh: boolean;
  fetchBalance: () => Promise<void>;
  checkIn: () => Promise<boolean>;
  manualRefresh: () => void;
}

export const CreditsContext = createContext<CreditsContextValue | undefined>(undefined);

export const CreditsProvider: React.FC<React.PropsWithChildren> = ({ children }) => {
  const { status, whoami, authState } = useCloudAuth();
  const isAuthenticated = status === 'authenticated' && !!whoami?.authenticated;
  const currentAccountId =
    whoami?.userId ||
    whoami?.email ||
    whoami?.username ||
    (authState.phase === 'authenticated' ? authState.accountId : undefined);

  const [mockCredits, setMockCreditsState] = useState<{ balance: number; authenticated: boolean } | null>(() => {
    const loaded = loadMockCredits();
    if (loaded !== null) return loaded;
    // Default mock balance of 500 in dev / mock-ready mode so the user can immediately observe the warning bubble
    return { balance: 500, authenticated: true };
  });

  const [balance, setBalance] = useState(0);
  const [authenticated, setAuthenticated] = useState(false);
  const [lastCheckInDayKey, setLastCheckInDayKey] = useState<number>(() => loadDayKey(currentAccountId));
  const [isFetchingBalance, setIsFetchingBalance] = useState(false);

  // Global mock change listener & window helper bindings
  useEffect(() => {
    if (typeof window === 'undefined') return;

    const handleMockChange = (e: Event) => {
      const customEvent = e as CustomEvent<{ balance: number | null; authenticated?: boolean }>;
      const { balance: newBalance, authenticated: newAuth = true } = customEvent.detail || {};
      saveMockCredits(newBalance, newAuth);
      setMockCreditsState(newBalance === null ? null : { balance: newBalance, authenticated: newAuth });
    };

    window.addEventListener('nomi:credits:mock:change', handleMockChange);

    // Global testing utilities available on window
    (window as unknown as { __setMockCredits: (bal: number | null, auth?: boolean) => void }).__setMockCredits = (
      bal: number | null,
      auth = true
    ) => {
      window.dispatchEvent(
        new CustomEvent('nomi:credits:mock:change', {
          detail: { balance: bal, authenticated: auth },
        })
      );
      console.info(
        `%c[Credits Mock]%c Set mock balance to: ${bal} (authenticated: ${auth}). Call window.__resetCreditsDismiss() if you closed bubbles today.`,
        'color: #f59e0b; font-weight: bold;',
        'color: inherit;'
      );
    };

    (window as unknown as { __resetCreditsDismiss: () => void }).__resetCreditsDismiss = () => {
      try {
        const prefix = 'nomifun:credits-bubble:dismiss:';
        const keysToRemove: string[] = [];
        for (let i = 0; i < localStorage.length; i++) {
          const k = localStorage.key(i);
          if (k && k.startsWith(prefix)) {
            keysToRemove.push(k);
          }
        }
        keysToRemove.forEach((k) => localStorage.removeItem(k));
        console.info(
          `%c[Credits Mock]%c Cleared ${keysToRemove.length} dismiss record(s). Bubbles are restored.`,
          'color: #10b981; font-weight: bold;',
          'color: inherit;'
        );
        window.dispatchEvent(
          new CustomEvent('nomi:credits:mock:change', {
            detail: {
              balance: mockCredits?.balance ?? 500,
              authenticated: mockCredits?.authenticated ?? true,
            },
          })
        );
      } catch (err) {
        console.error('Failed to reset credits dismiss:', err);
      }
    };

    return () => {
      window.removeEventListener('nomi:credits:mock:change', handleMockChange);
    };
  }, [mockCredits]);
  const [isCheckingIn, setIsCheckingIn] = useState(false);
  const [lastRefreshAt, setLastRefreshAt] = useState(0);
  const [cooldownSeconds, setCooldownSeconds] = useState(0);

  // Refs back the concurrency guards and the day-key check so the callbacks
  // keep stable identities (avoids re-firing the auto-refresh effect after a
  // successful check-in updates lastCheckInDayKey).
  const isFetchingBalanceRef = useRef(false);
  const isCheckingInRef = useRef(false);
  const pendingBalanceRefreshRef = useRef(false);
  const lastCheckInDayKeyRef = useRef(lastCheckInDayKey);
  const currentAccountIdRef = useRef(currentAccountId);

  useEffect(() => {
    lastCheckInDayKeyRef.current = lastCheckInDayKey;
  }, [lastCheckInDayKey]);

  useEffect(() => {
    currentAccountIdRef.current = currentAccountId;
  }, [currentAccountId]);

  const fetchBalance = useCallback(async () => {
    if (!isAuthenticated) return;
    if (isFetchingBalanceRef.current) {
      pendingBalanceRefreshRef.current = true;
      return;
    }
    isFetchingBalanceRef.current = true;
    setIsFetchingBalance(true);
    try {
      const result = await ipcBridge.media.getCredits.invoke();
      setBalance(result.balance);
      setAuthenticated(result.authenticated);
      setLastRefreshAt(Date.now());
    } catch (error) {
      console.warn('Credits balance fetch failed:', error);
    } finally {
      isFetchingBalanceRef.current = false;
      setIsFetchingBalance(false);
      if (pendingBalanceRefreshRef.current) {
        pendingBalanceRefreshRef.current = false;
        void fetchBalance();
      }
    }
  }, [isAuthenticated]);

  // Perform the daily check-in. Returns true only when a FRESH check-in ran
  // AND authoritatively set `balance` (server granted points this call). Returns
  // false when locally deduped, when the server reports `alreadyCheckedIn`
  // (balance may be a minimal/omitted payload → caller should fall back to a
  // balance fetch), or on failure. The server-confirmed `dayKey` is persisted on
  // any successful call so we never re-hit the endpoint same-day.
  const checkIn = useCallback(async (): Promise<boolean> => {
    if (!isAuthenticated || isCheckingInRef.current) return false;
    // Local dedup: once per local day per account. (The server dedups too.)
    const todayKey = getTodayKey();
    if (todayKey <= lastCheckInDayKeyRef.current) return false;
    // Snapshot the account now: if the user logs out and into another account
    // while this request is in flight, the grant still belongs to this one.
    const accountId = currentAccountIdRef.current;
    isCheckingInRef.current = true;
    setIsCheckingIn(true);
    try {
      const result = await ipcBridge.media.checkin.invoke({
        timeZone: getCurrentCronTimeZone(),
      });
      setAuthenticated(result.authenticated);
      const dayKey =
        typeof result.dayKey === 'number' && result.dayKey > 0 ? result.dayKey : todayKey;
      setLastCheckInDayKey(dayKey);
      lastCheckInDayKeyRef.current = dayKey;
      saveDayKey(dayKey, accountId);
      // Only a fresh grant carries a trustworthy balance. When the server says
      // alreadyCheckedIn (signed in elsewhere today while our local dayKey was
      // stale), the balance field may be omitted (serde defaults to 0) — don't
      // zero out the display; let the caller refresh via fetchBalance instead.
      if (!result.alreadyCheckedIn) {
        setBalance(result.balance);
        setLastRefreshAt(Date.now());
        return true;
      }
      return false;
    } catch (error) {
      console.warn('Daily check-in failed:', error);
      return false;
    } finally {
      isCheckingInRef.current = false;
      setIsCheckingIn(false);
    }
  }, [isAuthenticated]);

  // --- Manual refresh with 5s cooldown (bypasses scene throttle) -------------
  const cooldownTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const manualRefresh = useCallback(() => {
    if (!isAuthenticated || isFetchingBalanceRef.current || isCheckingInRef.current || cooldownSeconds > 0) return;
    setCooldownSeconds(Math.ceil(MANUAL_COOLDOWN_MS / 1000));
    const startedAt = Date.now();
    if (cooldownTimerRef.current) clearInterval(cooldownTimerRef.current);
    cooldownTimerRef.current = setInterval(() => {
      const remaining = Math.max(0, MANUAL_COOLDOWN_MS - (Date.now() - startedAt));
      setCooldownSeconds(Math.ceil(remaining / 1000));
      if (remaining <= 0 && cooldownTimerRef.current) {
        clearInterval(cooldownTimerRef.current);
        cooldownTimerRef.current = null;
        setCooldownSeconds(0);
      }
    }, 1000);

    const todayKey = getTodayKey();
    if (todayKey > lastCheckInDayKeyRef.current) {
      void checkIn().then((authoritative) => {
        if (!authoritative) void fetchBalance();
      });
    } else {
      void fetchBalance();
    }
  }, [isAuthenticated, cooldownSeconds, fetchBalance, checkIn]);

  // Clear the cooldown timer on unmount.
  useEffect(() => {
    return () => {
      if (cooldownTimerRef.current) clearInterval(cooldownTimerRef.current);
    };
  }, []);

  const canRefresh = isAuthenticated && !isFetchingBalance && cooldownSeconds === 0;

  // --- Account switch & Reset on logout ---------------------------------------
  useEffect(() => {
    if (!isAuthenticated) {
      setBalance(0);
      setAuthenticated(false);
      setLastRefreshAt(0);
      setLastCheckInDayKey(0);
      lastCheckInDayKeyRef.current = 0;
      if (cooldownTimerRef.current) {
        clearInterval(cooldownTimerRef.current);
        cooldownTimerRef.current = null;
      }
      setCooldownSeconds(0);
      globalLastTriggerByScene = { mount: 0, midnight: 0, online: 0, focus: 0, polling: 0 };
    } else {
      const storedKey = loadDayKey(currentAccountId);
      setLastCheckInDayKey(storedKey);
      lastCheckInDayKeyRef.current = storedKey;
    }
  }, [isAuthenticated, currentAccountId]);

  // --- Auto-refresh: mount + focus + polling + online + midnight -------------
  const triggerBalance = useCallback(
    (scene: RefreshScene) => {
      if (!isAuthenticated || isFetchingBalanceRef.current || isCheckingInRef.current) return;
      const now = Date.now();
      if (now - globalLastTriggerByScene[scene] < SCENE_INTERVAL_MS[scene]) return;
      globalLastTriggerByScene[scene] = now;
      // Only ONE balance writer per cycle, to avoid a fetch/checkin overwrite
      // race. If a check-in is due, let it own the balance (it carries the
      // post-grant total); otherwise fall back to a plain balance fetch. When
      // the check-in doesn't authoritatively set balance (already checked in /
      // failed), refresh via fetchBalance instead.
      const todayKey = getTodayKey();
      if (todayKey > lastCheckInDayKeyRef.current) {
        void checkIn().then((authoritative) => {
          if (!authoritative) void fetchBalance();
        });
      } else {
        void fetchBalance();
      }
    },
    [isAuthenticated, fetchBalance, checkIn]
  );

  useEffect(() => {
    if (!isAuthenticated) return;

    triggerBalance('mount');

    const onFocusOrVisible = () => {
      if (typeof document !== 'undefined' && document.hidden) return;
      triggerBalance('focus');
    };
    window.addEventListener('focus', onFocusOrVisible);
    document.addEventListener('visibilitychange', onFocusOrVisible);

    const onOnline = () => {
      triggerBalance('online');
    };
    window.addEventListener('online', onOnline);

    const intervalId = window.setInterval(() => {
      if (typeof document !== 'undefined' && document.hidden) return;
      triggerBalance('polling');
    }, POLLING_INTERVAL_MS);

    let midnightTimer: ReturnType<typeof setTimeout> | undefined;
    const scheduleMidnight = () => {
      if (midnightTimer) clearTimeout(midnightTimer);
      const delay = getMsUntilNextMidnight();
      midnightTimer = setTimeout(() => {
        triggerBalance('midnight');
        scheduleMidnight();
      }, delay);
    };
    scheduleMidnight();

    let consumptionTimer: ReturnType<typeof setTimeout> | undefined;
    const onConsumption = () => {
      if (consumptionTimer) clearTimeout(consumptionTimer);
      consumptionTimer = setTimeout(() => {
        const todayKey = getTodayKey();
        if (todayKey > lastCheckInDayKeyRef.current) {
          void checkIn().then((authoritative) => {
            if (!authoritative) void fetchBalance();
          });
        } else {
          void fetchBalance();
        }
      }, 800);
    };
    emitter.on('nomi.credits.balance.refresh', onConsumption);
    emitter.on('nomi.turn_credits.updated', onConsumption);

    return () => {
      window.removeEventListener('focus', onFocusOrVisible);
      document.removeEventListener('visibilitychange', onFocusOrVisible);
      window.removeEventListener('online', onOnline);
      window.clearInterval(intervalId);
      if (midnightTimer) clearTimeout(midnightTimer);
      if (consumptionTimer) clearTimeout(consumptionTimer);
      emitter.off('nomi.credits.balance.refresh', onConsumption);
      emitter.off('nomi.turn_credits.updated', onConsumption);
    };
  }, [isAuthenticated, currentAccountId, triggerBalance, fetchBalance, checkIn]);

  const effectiveBalance = mockCredits !== null ? mockCredits.balance : balance;
  const effectiveAuthenticated =
    mockCredits !== null ? mockCredits.authenticated : isAuthenticated && authenticated;
  const effectiveLastRefreshAt = mockCredits !== null ? (lastRefreshAt || 1) : lastRefreshAt;

  const value = useMemo<CreditsContextValue>(
    () => ({
      balance: effectiveBalance,
      authenticated: effectiveAuthenticated,
      lastCheckInDayKey,
      isFetchingBalance,
      isCheckingIn,
      lastRefreshAt: effectiveLastRefreshAt,
      cooldownSeconds,
      canRefresh,
      fetchBalance,
      checkIn,
      manualRefresh,
    }),
    [
      effectiveBalance,
      effectiveAuthenticated,
      lastCheckInDayKey,
      isFetchingBalance,
      isCheckingIn,
      effectiveLastRefreshAt,
      cooldownSeconds,
      canRefresh,
      fetchBalance,
      checkIn,
      manualRefresh,
    ]
  );

  return <CreditsContext.Provider value={value}>{children}</CreditsContext.Provider>;
};

export function useCredits(): CreditsContextValue {
  const context = useContext(CreditsContext);
  if (!context) {
    throw new Error('useCredits must be used within a CreditsProvider');
  }
  return context;
}

