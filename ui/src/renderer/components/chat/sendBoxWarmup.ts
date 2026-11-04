/**
 * Composer warmup admission.
 *
 * Focusing a chat composer pre-warms the conversation runtime. A voice call
 * mounts the same composer, but its runtime build already holds the
 * preparation gate; warming again from a voice call is pure contention (and
 * used to leave the dock waiting ~26s behind the gate). Voice sessions warm
 * through the call path instead.
 */
export function shouldWarmComposerOnFocus(args: {
  isVoiceConversation: boolean;
  conversationId?: string | null;
  warmedConversationId?: string | null;
}): boolean {
  const { isVoiceConversation, conversationId, warmedConversationId } = args;
  if (isVoiceConversation) return false;
  if (!conversationId) return false;
  return warmedConversationId !== conversationId;
}
