/**
 * Remember the last Video Generation workspace session so the sider can
 * restore the project detail instead of always bouncing to the list home.
 */

const LAST_SESSION_KEY = 'flowy.videoGeneration.lastSessionId';

export function rememberVideoGenerationSession(sessionId: string | null | undefined): void {
  const id = (sessionId ?? '').trim();
  if (!id) return;
  try {
    window.sessionStorage.setItem(LAST_SESSION_KEY, id);
  } catch {
    /* private mode / quota — ignore */
  }
}

export function clearVideoGenerationSessionMemory(sessionId?: string | null): void {
  try {
    if (sessionId) {
      const current = window.sessionStorage.getItem(LAST_SESSION_KEY);
      if (current && current !== sessionId) return;
    }
    window.sessionStorage.removeItem(LAST_SESSION_KEY);
  } catch {
    /* ignore */
  }
}

export function readRememberedVideoGenerationSession(): string | null {
  try {
    const id = window.sessionStorage.getItem(LAST_SESSION_KEY)?.trim() ?? '';
    return id || null;
  } catch {
    return null;
  }
}

/** Path for sider / deep-link restore: last workspace, else list home. */
export function videoGenerationEntryPath(): string {
  const id = readRememberedVideoGenerationSession();
  return id ? `/video-generation/${id}` : '/video-generation';
}
