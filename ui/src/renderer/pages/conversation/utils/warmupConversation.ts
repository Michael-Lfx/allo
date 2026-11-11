import type { ConversationId } from '@/common/types/ids';
import { ipcBridge } from '@/common';
import type { TChatConversation } from '@/common/config/storage';
import { getConversationOrNull } from './conversationCache';
import { getConversationRuntimeAuthority } from './conversationRuntime';
import { isVoiceConversation } from './isVoiceConversation';

const warmupByConversation = new Map<ConversationId, Promise<void>>();

export function warmupConversation(conversation_id: ConversationId): Promise<void> {
  const existing = warmupByConversation.get(conversation_id);
  if (existing) return existing;

  const promise = ipcBridge.conversation.warmup.invoke({ conversation_id }).finally(() => {
    warmupByConversation.delete(conversation_id);
  });
  warmupByConversation.set(conversation_id, promise);
  return promise;
}

/**
 * A voice conversation is owned by its acoustic binding, never by the chat
 * warmup path: it must stay read-only even when its durable status is Pending
 * and its runtime projection looks idle. Idle projections are not authority to
 * open a text turn there.
 */
export const shouldWarmupConversationOnPassiveMount = (
  conversation: Pick<TChatConversation, 'status' | 'runtime' | 'type' | 'extra'> | null
): boolean =>
  conversation?.status === 'pending' &&
  getConversationRuntimeAuthority(conversation) === 'idle' &&
  !isVoiceConversation(conversation);

type PassiveWarmupDependencies = {
  getConversation: (conversationId: ConversationId) => Promise<TChatConversation | null>;
  warmup: (conversationId: ConversationId) => Promise<void>;
};

const defaultPassiveWarmupDependencies: PassiveWarmupDependencies = {
  getConversation: getConversationOrNull,
  warmup: warmupConversation,
};

/**
 * Passive view mounting may prepare only an exact Pending, idle conversation.
 *
 * Finished and Running snapshots are terminal/owned authority respectively:
 * navigating back to either must remain a read-only hydration path and must
 * never issue a warmup POST. Explicit user actions may still call
 * `warmupConversation` directly.
 */
export async function warmupConversationForPassiveMount(
  conversationId: ConversationId,
  dependencies: PassiveWarmupDependencies = defaultPassiveWarmupDependencies
): Promise<boolean> {
  const conversation = await dependencies.getConversation(conversationId);
  if (!shouldWarmupConversationOnPassiveMount(conversation)) return false;

  await dependencies.warmup(conversationId);
  return true;
}
