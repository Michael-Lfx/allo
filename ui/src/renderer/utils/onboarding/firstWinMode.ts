

import { useCallback, useEffect, useState } from 'react';
import { hasFunnelEvent } from '@renderer/utils/analytics/productFunnel';

const FIRST_WIN_STORAGE_KEY = 'flowy.firstWin.completed.v1';
const FIRST_WIN_EVENT = 'flowy:first-win';

let memoryCompleted = false;

function canUseStorage(): boolean {
  return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined';
}

function readCompleted(): boolean {
  if (memoryCompleted) return true;
  if (!canUseStorage()) return false;
  try {
    if (window.localStorage.getItem(FIRST_WIN_STORAGE_KEY) === '1') {
      memoryCompleted = true;
      return true;
    }
  } catch {
    // ignore
  }
  return false;
}

/**
 * First-win ends only when the user confirms value (copy / follow-up /
 * outcome card), not when the first token or answer stream finishes.
 */
export function isFirstWinCompleted(): boolean {
  return readCompleted() || hasFunnelEvent('first_value_confirmed');
}

export function markFirstWinCompleted(): void {
  memoryCompleted = true;
  if (canUseStorage()) {
    try {
      window.localStorage.setItem(FIRST_WIN_STORAGE_KEY, '1');
    } catch {
      // ignore
    }
    window.dispatchEvent(new CustomEvent(FIRST_WIN_EVENT, { detail: { completed: true } }));
  }
}

export function resetFirstWinForTests(): void {
  memoryCompleted = false;
  if (!canUseStorage()) return;
  try {
    window.localStorage.removeItem(FIRST_WIN_STORAGE_KEY);
  } catch {
    // ignore
  }
}

/**
 * First-time users stay in a focused "first win" stage until they confirm a
 * reviewable result. Returning users see the full workstation immediately.
 */
export function useFirstWinMode(): {
  isFirstWin: boolean;
  completeFirstWin: () => void;
} {
  const [completed, setCompleted] = useState(() => isFirstWinCompleted());

  useEffect(() => {
    const sync = () => setCompleted(isFirstWinCompleted());
    if (typeof window === 'undefined') return undefined;
    window.addEventListener(FIRST_WIN_EVENT, sync);
    window.addEventListener('flowy:funnel', sync);
    return () => {
      window.removeEventListener(FIRST_WIN_EVENT, sync);
      window.removeEventListener('flowy:funnel', sync);
    };
  }, []);

  const completeFirstWin = useCallback(() => {
    markFirstWinCompleted();
    setCompleted(true);
  }, []);

  return {
    isFirstWin: !completed,
    completeFirstWin,
  };
}
