import type {VoiceSnapshot} from '@common/adapter/voiceCompanion';
import type {ConversationId} from '@common/types/ids';
import type {TMessage} from '@common/chat/chatLib';
import {commitAuthoritativeConversationReset} from '../conversation/Messages/conversationMessageCoordinator';

// One acoustic call owns the volatile three-turn window. Keep only reset
// identities across view teardown, never a second cache of message contents.
let resetSequence = 0;
let completedReset = 0;
let invalidatedWindow: {scope: string; turns: ReadonlySet<string>} | null = null;
const voiceScope = (snapshot: VoiceSnapshot, conversationId: string) =>
  `${conversationId}:${snapshot.instance_id}:${snapshot.session_id}`;

/** Capture before the async reset; commit only after the authoritative reset succeeds. */
export function prepareVoiceHistoryReset(snapshot: VoiceSnapshot | null, conversationId: ConversationId): () => void {
  // Without a known scope, a successful reset could accept the old window later.
  if (!snapshot || (snapshot.session_id && (!snapshot.binding || snapshot.binding.session_id !== snapshot.session_id))) {
    throw new Error('voice_reset_scope_unavailable');
  }
  const operation = ++resetSequence;
  const bound = snapshot?.session_id && snapshot.binding?.session_id === snapshot.session_id && snapshot.binding.conversation_id === conversationId;
  const window = bound ? {
    scope: voiceScope(snapshot, conversationId),
    turns: new Set((snapshot.conversation ?? []).filter(turn => turn.session_id === snapshot.session_id).slice(-3).map(turn => turn.turn_id)),
  } : null;
  return () => {
    commitAuthoritativeConversationReset(conversationId);
    if (window && operation > completedReset) {
      completedReset = operation;
      invalidatedWindow = window;
    }
  };
}

/** Both reset APIs freeze voice identities before changing authoritative history. */
export async function resetVoiceHistory<T>(snapshot: VoiceSnapshot | null, conversationId: ConversationId, reset: () => Promise<T>): Promise<T> {
  const commit = prepareVoiceHistoryReset(snapshot, conversationId);
  const result = await reset();
  commit();
  return result;
}

const invalidatedTurns = (snapshot: VoiceSnapshot | null, conversationId: string): ReadonlySet<string> | undefined =>
  snapshot && invalidatedWindow?.scope === voiceScope(snapshot, conversationId) ? invalidatedWindow.turns : undefined;

export interface LiveVoiceOptions {
  instanceId?: string;
  retiredIds?: ReadonlySet<string>;
  blockedTurns?: ReadonlySet<string>;
  waitingTurns?: ReadonlySet<string>;
}
export interface LiveVoiceHistory {
  scope: string;
  instanceId: string;
  epoch: number;
  retiredIds: Set<string>;
  blockedTurns: Set<string>;
  receipts: Map<string, number>;
  observedTurns: Set<string>;
  waitingTurns: Set<string>;
}
export const createLiveVoiceHistory = (): LiveVoiceHistory => ({
  scope: '', instanceId: '', epoch: 0, retiredIds: new Set(), blockedTurns: new Set(), receipts: new Map(), observedTurns: new Set(), waitingTurns: new Set(),
});

/** Retirement remains view-local; successful resets also consult the bounded call fence. */
export function reconcileLiveVoiceHistory(previous: LiveVoiceHistory, args: {
  snapshot: VoiceSnapshot | null; conversationId: string; epoch: number;
  visibleIds: ReadonlySet<string>; currentSequence: number;
  latestPage: {conversationId: string; epoch: number; sequence: number} | null;
}): {state: LiveVoiceHistory; refreshKey: string} {
  const {snapshot, conversationId, epoch, visibleIds, currentSequence, latestPage} = args;
  const scope = `${conversationId}:${snapshot?.instance_id ?? ''}:${snapshot?.session_id ?? ''}`;
  const sameScope = previous.scope === scope;
  const turns = snapshot?.binding?.conversation_id === conversationId
    ? (snapshot.conversation ?? []).filter(turn => turn.session_id && turn.session_id === snapshot.session_id) : [];
  const blockedTurns = sameScope ? new Set(previous.blockedTurns) : new Set<string>();
  invalidatedTurns(snapshot, conversationId)?.forEach(turn => blockedTurns.add(turn));
  if (sameScope && previous.epoch !== epoch) {
    // A new utterance can arrive in the same commit as the reset's history read.
    [...previous.observedTurns, ...previous.waitingTurns].forEach(turn => blockedTurns.add(turn));
  }
  const activeIds = new Set(turns.flatMap(turn => [turn.user_message_id, turn.assistant_message_id].filter((id): id is string => Boolean(id))));
  const state: LiveVoiceHistory = {
    scope, instanceId: snapshot?.instance_id ?? '', epoch, blockedTurns,
    retiredIds: new Set(sameScope && previous.epoch === epoch ? [...previous.retiredIds].filter(id => activeIds.has(id)) : []),
    receipts: new Map(sameScope && previous.epoch === epoch ? [...previous.receipts].filter(([id]) => activeIds.has(id)) : []),
    observedTurns: new Set(sameScope && previous.epoch === epoch ? [...previous.observedTurns].filter(id=>turns.some(turn=>turn.turn_id===id)) : []),
    waitingTurns: new Set(),
  };
  const projected = new Set(snapshot?.projected_turn_ids ?? []);
  const missing: string[] = [];
  for (const turn of turns) {
    if (blockedTurns.has(turn.turn_id)) continue;
    if (!projected.has(turn.turn_id) || [turn.user_message_id,turn.assistant_message_id].some(id=>id&&visibleIds.has(id))) state.observedTurns.add(turn.turn_id);
    // On remount, a completed cached turn may be outside the newest history page.
    // Do not first append it to the tail while verifying its receipt. A turn
    // already shown live in this view stays visible throughout that same GET.
    if (projected.has(turn.turn_id) && !state.observedTurns.has(turn.turn_id)) state.waitingTurns.add(turn.turn_id);
    for (const id of [turn.user_message_id, turn.assistant_message_id]) {
      if (!id) continue;
      if (visibleIds.has(id)) state.retiredIds.add(id);
      if (!projected.has(turn.turn_id) || state.retiredIds.has(id)) continue;
      const observedAt = state.receipts.get(id) ?? currentSequence;
      state.receipts.set(id, observedAt);
      if (latestPage?.conversationId === conversationId && latestPage.epoch === epoch && latestPage.sequence > observedAt) {
        // A successful post-receipt newest-page read either supplied the row or proved it is outside that window.
        state.retiredIds.add(id);
      } else missing.push(id);
    }
  }
  // Worker keeps three turns; bound local retirement and invalidation state to that same window.
  state.blockedTurns = new Set([...blockedTurns].filter(id => turns.some(turn => turn.turn_id === id)));
  return {state, refreshKey: missing.length ? `${scope}:${epoch}:${missing.sort().join(',')}` : ''};
}

/** Live presentation only; durable rows continue to belong to Conversation. */
export function liveVoiceTurn(snapshot: VoiceSnapshot | null, conversationId: string,
  visibleIds: ReadonlySet<string> = new Set(), options: LiveVoiceOptions = {}): VoiceSnapshot | null {
  if (!snapshot || (options.instanceId && snapshot.instance_id !== options.instanceId)
    || snapshot.binding?.conversation_id !== conversationId || snapshot.binding.session_id !== snapshot.session_id) return null;
  const projected = new Set(snapshot.projected_turn_ids ?? []);
  const invalidated = invalidatedTurns(snapshot, conversationId);
  const turns = (snapshot.conversation ?? []).flatMap(turn => {
    if (!turn.session_id || turn.session_id !== snapshot.session_id || invalidated?.has(turn.turn_id) || options.blockedTurns?.has(turn.turn_id) || options.waitingTurns?.has(turn.turn_id)) return [];
    const hasIdentity = Boolean(turn.user_message_id || turn.assistant_message_id);
    if (!hasIdentity && projected.has(turn.turn_id)) return [];
    const retired = (id: string | null | undefined) => Boolean(id && (visibleIds.has(id) || options.retiredIds?.has(id)));
    const user_text = retired(turn.user_message_id) ? null : turn.user_text;
    const assistant_text = retired(turn.assistant_message_id) ? null : turn.assistant_text;
    return user_text || assistant_text || (turn.turn_id === snapshot.turn_id && snapshot.phase === 'transcribing')
      ? [{...turn, user_text, assistant_text}] : [];
  });
  if (!turns.length) return null;
  const current = turns.find(turn => turn.turn_id === snapshot.turn_id);
  return {...snapshot, conversation: turns, user_text: current?.user_text ?? null, assistant_text: current?.assistant_text ?? null};
}

/** Ordinary rows for the current live scope, kept outside durable message caches. */
export function buildVoiceLiveMessages(snapshot: VoiceSnapshot | null, conversationId: ConversationId,
  persisted: readonly TMessage[], options?: LiveVoiceOptions): TMessage[] {
  const ids = new Set(persisted.flatMap(message => [message.message_id, message.msg_id].filter((id): id is NonNullable<typeof id> => Boolean(id))));
  const live = liveVoiceTurn(snapshot, conversationId, ids, options);
  const baseTime = Math.max(0, ...persisted.map(message => message.created_at ?? 0));
  return (live?.conversation ?? []).flatMap((turn, index): TMessage[] => {
    const make = (content: string, position: 'left' | 'right'): TMessage => ({
      id: `voice-live:${conversationId}:${snapshot?.session_id ?? ''}:${turn.turn_id}:${position}`,
      conversation_id: conversationId, type: 'text', content: {content}, position, status: 'pending',
      created_at: baseTime + index * 2 + (position === 'right' ? 1 : 2),
    });
    return [...(turn.user_text ? [make(turn.user_text, 'right')] : []), ...(turn.assistant_text ? [make(turn.assistant_text, 'left')] : [])];
  });
}

/** Place an unfinished side beside its own durable partner, even when a later turn landed first. */
export function mergeVoiceTranscript(snapshot: VoiceSnapshot|null, conversationId:ConversationId,
  persisted:readonly TMessage[], options?:LiveVoiceOptions): TMessage[] {
  const result=[...persisted];
  // Durable rows are the source of truth. Never rewrite another binding,
  // session, cleared turn or stale instance from a live snapshot.
  const scoped = Boolean(snapshot && (!options?.instanceId || snapshot.instance_id === options.instanceId)
    && snapshot.binding?.conversation_id === conversationId
    && snapshot.binding.session_id === snapshot.session_id);
  if (!scoped) return result as TMessage[];
  const turns=snapshot!.conversation ?? [];
  const pending=buildVoiceLiveMessages(snapshot,conversationId,result,options);
  if (!pending.length) return result as TMessage[];
  const indexOf=(id:string|null|undefined)=>id?result.findIndex(row=>row.message_id===id||row.msg_id===id):-1;
  turns.forEach((turn,index)=>{
    const prefix=`voice-live:${conversationId}:${snapshot?.session_id ?? ''}:${turn.turn_id}:`;
    const rows=pending.filter(row=>row.id.startsWith(prefix));
    if (!rows.length) return;
    const user=indexOf(turn.user_message_id),assistant=indexOf(turn.assistant_message_id);
    const nextAnchor=turns.slice(index+1).flatMap(next=>[indexOf(next.user_message_id),indexOf(next.assistant_message_id)]).find(position=>position>=0);
    const position=user>=0?user+1:assistant>=0?assistant:nextAnchor??result.length;
    result.splice(position,0,...rows);
  });
  return result;
}
