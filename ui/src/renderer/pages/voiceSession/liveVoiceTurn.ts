import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';
import type {ConversationId} from '@common/types/ids';
import type {TMessage} from '@common/chat/chatLib';

/** Live presentation only; durable rows continue to belong to Conversation. */
export function liveVoiceTurn(snapshot: VoiceSnapshot | null, conversationId: string, visibleIds: ReadonlySet<string> = new Set()): VoiceSnapshot | null {
  if (!snapshot || snapshot.binding?.conversation_id !== conversationId || snapshot.binding.session_id !== snapshot.session_id) return null;
  const projected = new Set(snapshot.projected_turn_ids ?? []);
  const turns = (snapshot.conversation ?? []).flatMap(turn => {
    const hasIdentity = Boolean(turn.user_message_id || turn.assistant_message_id);
    if (!hasIdentity && projected.has(turn.turn_id)) return [];
    const user_text = turn.user_message_id && visibleIds.has(turn.user_message_id) ? null : turn.user_text;
    const assistant_text = turn.assistant_message_id && visibleIds.has(turn.assistant_message_id) ? null : turn.assistant_text;
    return user_text || assistant_text || (turn.turn_id === snapshot.turn_id && snapshot.phase === 'transcribing')
      ? [{...turn, user_text, assistant_text}] : [];
  });
  if (!turns.length) return null;
  const current = turns.find(turn => turn.turn_id === snapshot.turn_id);
  return { ...snapshot, conversation: turns, user_text: current?.user_text ?? null, assistant_text: current?.assistant_text ?? null };
}

/** Render live text through the ordinary message pipeline without putting it in durable caches. */
export function buildVoiceLiveMessages(snapshot: VoiceSnapshot | null, conversationId: ConversationId, persisted: readonly TMessage[]): TMessage[] {
  const ids = new Set(persisted.flatMap(message => [message.message_id, message.msg_id].filter((id): id is NonNullable<typeof id> => Boolean(id))));
  const live = liveVoiceTurn(snapshot, conversationId, ids);
  const baseTime = Math.max(0, ...persisted.map(message => message.created_at ?? 0));
  return (live?.conversation ?? []).flatMap((turn, index): TMessage[] => {
    const make = (content: string, position: 'left'|'right'): TMessage => ({
      id:`voice-live:${turn.turn_id}:${position}`, conversation_id:conversationId, type:'text',
      content:{content}, position, status:'pending', created_at:baseTime + index*2 + (position==='right'?1:2),
    });
    return [...(turn.user_text ? [make(turn.user_text,'right')] : []), ...(turn.assistant_text ? [make(turn.assistant_text,'left')] : [])];
  });
}
