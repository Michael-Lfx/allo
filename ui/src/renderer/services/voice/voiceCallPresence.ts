/**
 * Shared voice call presence & window synchronization strategy (CP-10, CP-11, CP-15).
 * Unifies session dismissal and companion owner resolution across the main window
 * and desktop companion windows.
 */

const STORAGE_KEY = 'flowy:voice_dismissed_sessions';
export const VOICE_DISMISSED_SESSIONS_EVENT = 'voice-dismissed-sessions-changed';

const inMemoryDismissed = new Set<string>();

export function getDismissedVoiceSessions(): Set<string> {
  if (typeof window === 'undefined' || !window.localStorage) {
    return inMemoryDismissed;
  }
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return inMemoryDismissed;
    const parsed = JSON.parse(raw);
    const set = new Set(Array.isArray(parsed) ? parsed : []);
    for (const item of inMemoryDismissed) set.add(item);
    return set;
  } catch {
    return inMemoryDismissed;
  }
}

export function dismissVoiceSession(sessionId: string): void {
  if (!sessionId) return;
  inMemoryDismissed.add(sessionId);
  if (typeof window !== 'undefined' && window.localStorage) {
    try {
      const current = getDismissedVoiceSessions();
      current.add(sessionId);
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(Array.from(current)));
      window.dispatchEvent(new Event(VOICE_DISMISSED_SESSIONS_EVENT));
    } catch {
      // Ignore storage quota or access errors
    }
  }
}

export function isVoiceSessionDismissed(sessionId: string | null | undefined): boolean {
  if (!sessionId) return false;
  return getDismissedVoiceSessions().has(sessionId);
}

/**
 * Resolves which companion ID should surface for this voice session.
 * Unified between main window `useCompanionWindowsSync` and companion window `index.tsx`.
 * Requires an explicit host-frozen owner; returns null if missing so no arbitrary companion
 * window claims false ownership.
 */
export function resolveVoiceOwnerCompanionId(
  memoryOwnerId: string | null | undefined
): string | null {
  const explicit = memoryOwnerId?.trim();
  if (explicit) {
    return explicit;
  }
  return null;
}
