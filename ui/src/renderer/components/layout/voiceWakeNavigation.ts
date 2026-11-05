/**
 * Voice wake must navigate exactly once per call activation.
 *
 * The daemon publishes a snapshot on every phase/emit change. Navigating on
 * every active snapshot steals the route back from a user who switched to
 * another conversation mid-call. The session id marks activation: a new call
 * session (wake / incoming call) may navigate; the same active session must
 * never yank the route again.
 */
export interface VoiceSessionRouteSnapshot {
  enabled?: boolean | null;
  connection?: string | null;
  phase?: string | null;
  self_check_active?: boolean | null;
  session_id?: string | null;
  binding?: { conversation_id?: string | null } | null;
}

const INACTIVE_PHASES = new Set(['idle', 'error']);

export function shouldFollowVoiceSession(
  previousSessionId: string | null | undefined,
  snapshot: VoiceSessionRouteSnapshot | null | undefined
): string | null {
  if (!snapshot) return null;
  if (snapshot.self_check_active) return null;
  if (!snapshot.enabled) return null;
  if (snapshot.connection !== 'connected') return null;

  const phase = snapshot.phase;
  if (!phase || INACTIVE_PHASES.has(phase)) return null;

  const conversationId = snapshot.binding?.conversation_id;
  if (!conversationId) return null;

  const sessionId = snapshot.session_id ?? null;
  if (!sessionId) return null;
  if (sessionId === (previousSessionId ?? null)) return null;
  return `/conversation/${encodeURIComponent(conversationId)}`;
}

/**
 * Advances the followed-session marker only when the snapshot has a real
 * binding target. An unbound active frame is a normal intermediate state; it
 * must not consume the activation, otherwise the later bound frame is ignored.
 */
export function nextVoiceWakeNavigation(
  previousSessionId: string | null | undefined,
  snapshot: VoiceSessionRouteSnapshot | null | undefined,
  currentPath: string
): { followedSessionId: string | null; target: string | null } {
  if (isVoiceSessionInactive(snapshot)) {
    return { followedSessionId: null, target: null };
  }
  const route = shouldFollowVoiceSession(previousSessionId, snapshot);
  if (!route) {
    return { followedSessionId: previousSessionId ?? null, target: null };
  }
  return {
    followedSessionId: snapshot?.session_id ?? null,
    target: route === currentPath ? null : route,
  };
}

/** True when the snapshot ended or went back to idle, so the next activation counts as new. */
export function isVoiceSessionInactive(
  snapshot: VoiceSessionRouteSnapshot | null | undefined
): boolean {
  if (!snapshot) return true;
  if (snapshot.self_check_active) return true;
  const phase = snapshot.phase;
  return !phase || INACTIVE_PHASES.has(phase);
}
