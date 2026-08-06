import type { ConversationId } from '@/common/types/ids';

/**
 * Gate between the Guid send flow and the destination page's initial-message
 * consumption.
 *
 * Guid registers the background "apply advanced config" work (knowledge / IDMM
 * / AutoWork, and goal-mode goalAction) right after `conversation.create`
 * resolves, then navigates immediately instead of blocking the transition on
 * those extra round-trips. The destination awaits the gate before POSTing the
 * first turn, preserving the invariant that configuration is live before the
 * runtime sees the first message.
 */

/** Leak guard: a hung gate promise must not pin the map entry forever. */
const CONFIG_GATE_TTL_MS = 60_000;

const gates = new Map<ConversationId, Promise<void>>();
const timers = new Map<ConversationId, ReturnType<typeof setTimeout>>();

export function registerConversationConfig(conversationId: ConversationId, work: Promise<void>): void {
  const key = conversationId;

  // Defensive: re-registering the same id clears the previous timer (ids are
  // freshly minted per send, so this should never happen in practice).
  const previousTimer = timers.get(key);
  if (previousTimer !== undefined) clearTimeout(previousTimer);

  const timer = setTimeout(() => {
    gates.delete(key);
    timers.delete(key);
  }, CONFIG_GATE_TTL_MS);

  const tracked = work
    .catch((error) => {
      // applyToConversation already never throws and goalAction has its own
      // try/catch — this is belt-and-braces against an unhandled rejection.
      console.error('[conversationConfigGate] Background conversation config failed:', error);
    })
    .finally(() => {
      clearTimeout(timer);
      timers.delete(key);
      gates.delete(key);
    });

  gates.set(key, tracked);
  timers.set(key, timer);
}

/**
 * Resolves with the registered config work, or immediately when nothing is
 * registered — which covers direct-link mounts (no guid send in flight) and
 * late awaits after the work already settled (config is applied by then).
 */
export function awaitConversationConfig(conversationId: ConversationId): Promise<void> {
  return gates.get(conversationId) ?? Promise.resolve();
}
