const INSTALL_ID_KEY = 'flowy.install_id.v1';

let memoryInstallId: string | null = null;

function canUseStorage(): boolean {
  return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined';
}

function createInstallId(): string {
  if (typeof globalThis.crypto?.randomUUID === 'function') {
    return globalThis.crypto.randomUUID();
  }
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export function getInstallId(): string {
  if (memoryInstallId) return memoryInstallId;
  if (canUseStorage()) {
    try {
      const existing = window.localStorage.getItem(INSTALL_ID_KEY);
      if (existing && existing.length >= 8) {
        memoryInstallId = existing;
        return existing;
      }
    } catch {
      // fall through
    }
  }
  const next = createInstallId();
  memoryInstallId = next;
  if (canUseStorage()) {
    try {
      window.localStorage.setItem(INSTALL_ID_KEY, next);
    } catch {
      // ignore
    }
  }
  return next;
}

/** Prefer the backend-persisted client id so heartbeats and PostHog share one key. */
export function adoptBackendClientId(clientId: string): string {
  const trimmed = clientId.trim();
  if (!trimmed) return getInstallId();
  memoryInstallId = trimmed;
  if (canUseStorage()) {
    try {
      window.localStorage.setItem(INSTALL_ID_KEY, trimmed);
    } catch {
      // ignore
    }
  }
  return trimmed;
}

export function resetInstallIdForTests(): void {
  memoryInstallId = null;
  if (!canUseStorage()) return;
  try {
    window.localStorage.removeItem(INSTALL_ID_KEY);
  } catch {
    // ignore
  }
}
