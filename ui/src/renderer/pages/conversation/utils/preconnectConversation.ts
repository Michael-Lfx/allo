import type { ConversationId } from '@/common/types/ids';
import { ipcBridge } from '@/common';

export const PRECONNECT_INTERVAL_MS = 30_000;

const lastPreconnectAt = new Map<ConversationId, number>();

type Preconnect = (conversationId: ConversationId) => Promise<unknown>;

const defaultPreconnect: Preconnect = (conversation_id) => ipcBridge.conversation.preconnect.invoke({ conversation_id });

/**
 * The provider gateway drops idle keep-alive sockets after roughly a minute, so
 * the first model call of a turn usually pays a fresh TLS handshake. Composing
 * a message is the earliest reliable signal that a send is coming.
 */
export function preconnectConversation(
  conversationId: ConversationId,
  now: number = Date.now(),
  preconnect: Preconnect = defaultPreconnect
): boolean {
  const last = lastPreconnectAt.get(conversationId);
  if (last !== undefined && now - last < PRECONNECT_INTERVAL_MS) return false;
  lastPreconnectAt.set(conversationId, now);
  void preconnect(conversationId).catch(() => {});
  return true;
}
