import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';

/** Live presentation only; durable rows continue to belong to Conversation. */
export function liveVoiceTurn(snapshot: VoiceSnapshot | null, conversationId: string): VoiceSnapshot | null {
  if (!snapshot || snapshot.binding?.conversation_id !== conversationId || snapshot.binding.session_id !== snapshot.session_id) return null;
  const projected = new Set(snapshot.projected_turn_ids ?? []);
  const turns = (snapshot.conversation ?? []).filter(turn => !projected.has(turn.turn_id) &&
    (Boolean(turn.user_text) || (turn.turn_id === snapshot.turn_id && snapshot.phase === 'transcribing')));
  if (!turns.length) return null;
  const current = turns.find(turn => turn.turn_id === snapshot.turn_id);
  return { ...snapshot, conversation: turns, user_text: current?.user_text ?? null, assistant_text: current?.assistant_text ?? null };
}
