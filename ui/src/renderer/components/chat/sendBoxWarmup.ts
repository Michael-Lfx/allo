/**
 * Composer warmup admission.
 *
 * Focusing a chat composer pre-warms the conversation runtime. A voice call
 * mounts the same composer, but its runtime build already holds the
 * preparation gate; warming again from a voice call is pure contention (and
 * used to leave the dock waiting ~26s behind the gate). Voice sessions warm
 * through the call path instead.
 */
export type ComposerVoiceKind = 'voice' | 'ordinary' | 'unknown';

export function shouldWarmComposerOnFocus(args: {
  isVoiceConversation: boolean;
  /** Tri-state from the conversation classifier; `unknown` must defer, never warm. */
  voiceConversationKind?: ComposerVoiceKind;
  conversationId?: string | null;
  warmedConversationId?: string | null;
}): boolean {
  const { isVoiceConversation, voiceConversationKind, conversationId, warmedConversationId } = args;
  if (voiceConversationKind && voiceConversationKind !== 'ordinary') return false;
  if (isVoiceConversation) return false;
  if (!conversationId) return false;
  return warmedConversationId !== conversationId;
}
