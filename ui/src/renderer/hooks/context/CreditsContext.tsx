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
import { useCloudAuth } from './CloudAuthContext';

// --- Day key (local midnight, YYYYMMDD integer) ------------------------------
// Mirrors FlowyClaw's getTodayKey(): the client-side gate flips at the user's
// local midnight. The server independently resolves the real day boundary from
// the posted `timeZone`, so the authoritative day key comes back in the
// response and is what we persist.
function getTodayKey(): number {
  const d = new Date();
  return Number(
    `${d.getFullYear()}${String(d.getMonth() + 1).padStart(2, '0')}${String(
      d.getDate()
    ).padStart(2, '0')}`
  );
}

// --- Persistence for lastCheckInDayKey ---------------------------------------
// No state library here (unlike FlowyClaw's zustand-persist); a plain key under
// the `nomifun` root prefix mirrors existing plain-key prefs. Deliberately NOT
// generation-scoped: the day key is a per-account fact, not per backend dataset.
const DAYKEY_STORAGE = 'nomifun:credits:lastCheckInDayKey';
function loadDayKey(): number {
  try {
    return Number(localStorage.getItem(DAYKEY_STORAGE)) || 0;
  } catch {
    return 0;
  }
}
function saveDayKey(key: number): void {
  try {
    localStorage.setItem(DAYKEY_STORAGE, String(key));
  } catch {
    // ignore storage failures (private mode, quota, etc.)
  }
}

// --- Auto-refresh scene throttle ---------------------------------------------
// Shared across the runtime so multiple consumers can't fan out duplicate
// requests. `mount` has no throttle; `focus`/`polling` are rate-limited.
export type RefreshScene = 'mount' | 'focus' | 'polling';
const SCENE_INTERVAL_MS: Record<RefreshScene, number> = {
  mount: 0,
  focus: 15_000,
  polling: 10 * 60_000,
};
const POLLING_INTERVAL_MS = 10 * 60_000;
const MANUAL_COOLDOWN_MS = 5_000;

let globalLastTriggerByScene: Record<RefreshScene, number> = {
  mount: 0,
  focus: 0,
  polling: 0,
};

// --- Context value -----------------------------------------------------------
interface CreditsContextValue {
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

const CreditsContext = createContext<CreditsContextValue | undefined>(undefined);

export const CreditsProvider: React.FC<React.PropsWithChildren> = ({ children }) => {
  const { status, whoami } = useCloudAuth();
  const isAuthenticated = status === 'authenticated' && !!whoami?.authenticated;

  const [balance, setBalance] = useState(0);
  const [authenticated, setAuthenticated] = useState(false);
  const [lastCheckInDayKey, setLastCheckInDayKey] = useState<number>(() => loadDayKey());
  const [isFetchingBalance, setIsFetchingBalance] = useState(false);
  const [isCheckingIn, setIsCheckingIn] = useState(false);
  const [lastRefreshAt, setLastRefreshAt] = useState(0);
  const [cooldownSeconds, setCooldownSeconds] = useState(0);

  // Refs back the concurrency guards and the day-key check so the callbacks
  // keep stable identities (avoids re-firing the auto-refresh effect after a
  // successful check-in updates lastCheckInDayKey).
  const isFetchingBalanceRef = useRef(false);
  const isCheckingInRef = useRef(false);
  const lastCheckInDayKeyRef = useRef(lastCheckInDayKey);
  useEffect(() => {
    lastCheckInDayKeyRef.current = lastCheckInDayKey;
  }, [lastCheckInDayKey]);

  const fetchBalance = useCallback(async () => {
    if (!isAuthenticated || isFetchingBalanceRef.current) return;
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
    // Local dedup: once per local day. (The server dedups too.) This assumes
    // the server returns dayKey as a YYYYMMDD integer matching getTodayKey();
    // if the formats ever diverge, local dedup silently no-ops and the
    // server-side dedup remains the safety net.
    const todayKey = getTodayKey();
    if (todayKey <= lastCheckInDayKeyRef.current) return false;
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
      saveDayKey(dayKey);
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
    if (!isAuthenticated || isFetchingBalanceRef.current || cooldownSeconds > 0) return;
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
    void fetchBalance();
  }, [isAuthenticated, cooldownSeconds, fetchBalance]);

  // Clear the cooldown timer on unmount.
  useEffect(() => {
    return () => {
      if (cooldownTimerRef.current) clearInterval(cooldownTimerRef.current);
    };
  }, []);

  const canRefresh = isAuthenticated && !isFetchingBalance && cooldownSeconds === 0;

  // --- Reset on logout -------------------------------------------------------
  useEffect(() => {
    if (!isAuthenticated) {
      setBalance(0);
      setAuthenticated(false);
      setLastRefreshAt(0);
      if (cooldownTimerRef.current) {
        clearInterval(cooldownTimerRef.current);
        cooldownTimerRef.current = null;
      }
      setCooldownSeconds(0);
      globalLastTriggerByScene = { mount: 0, focus: 0, polling: 0 };
    }
  }, [isAuthenticated]);

  // --- Auto-refresh: mount + window focus + 10min polling --------------------
  // The provider is the single always-mounted consumer, so this is the only
  // place the auto-refresh listeners live.
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

    const onFocus = () => triggerBalance('focus');
    window.addEventListener('focus', onFocus);

    // Skip polling while the window is hidden (backgrounded) to avoid wasteful
    // requests — visibility is re-acquired via the focus listener. Mirrors the
    // agent-refresh visibility gate in main.tsx.
    const intervalId = window.setInterval(() => {
      if (document.hidden) return;
      triggerBalance('polling');
    }, POLLING_INTERVAL_MS);

    return () => {
      window.removeEventListener('focus', onFocus);
      window.clearInterval(intervalId);
    };
  }, [isAuthenticated, triggerBalance]);

  const value = useMemo<CreditsContextValue>(
    () => ({
      balance,
      authenticated,
      lastCheckInDayKey,
      isFetchingBalance,
      isCheckingIn,
      lastRefreshAt,
      cooldownSeconds,
      canRefresh,
      fetchBalance,
      checkIn,
      manualRefresh,
    }),
    [
      balance,
      authenticated,
      lastCheckInDayKey,
      isFetchingBalance,
      isCheckingIn,
      lastRefreshAt,
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
