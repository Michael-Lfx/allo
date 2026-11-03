/** Match the backend's persisted markers; names, models and workspace paths are not type tags. */
export function isVoiceConversation(conversation: {type?:string;extra?:unknown} | null | undefined): boolean {
  if (conversation?.type !== 'nomi' || !conversation.extra || typeof conversation.extra !== 'object') return false;
  const extra = conversation.extra as Record<string, unknown>;
  return extra.voice === true
    || extra.is_default_voice === true
    || extra.is_voice_conversation === true
    || extra.session_mode === 'voice'
    || extra.session_mode === 'companion';
}
