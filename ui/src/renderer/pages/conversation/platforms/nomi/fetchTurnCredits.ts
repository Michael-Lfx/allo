/**
 * Fetch Flowy per-turn credit usage after an Agent Run ends and persist it
 * on the conversation for rehydrate + emit for live MessageText chips.
 */

import { ipcBridge } from '@/common';
import type { ConversationId, MessageId } from '@/common/types/ids';
import { FLOWY_BUILTIN_PROVIDER_ID, parseProviderId } from '@/common/types/ids';
import type { TChatConversation, TurnCreditUsageData } from '@/common/config/storage';
import { getConversationOrNull, refreshConversationCache } from '@/renderer/pages/conversation/utils/conversationCache';
import { emitter } from '@/renderer/utils/emitter';
import { mutate } from 'swr';

const MAX_TURN_CREDIT_ENTRIES = 40;

/** Survives MessageText remounts (first-turn list reconcile races the emit). */
const turnCreditsMemory = new Map<string, TurnCreditUsageData>();
const inflightFetches = new Map<string, Promise<TurnCreditUsageData | null>>();

function memoryKey(conversationId: string, turnId: string): string {
  return `${conversationId}\0${turnId}`;
}

export function peekTurnCredits(
  conversationId: ConversationId | string,
  turnId: MessageId | string
): TurnCreditUsageData | null {
  return turnCreditsMemory.get(memoryKey(String(conversationId), String(turnId))) ?? null;
}

function rememberTurnCredits(
  conversationId: ConversationId | string,
  turnId: MessageId | string,
  usage: TurnCreditUsageData
): void {
  turnCreditsMemory.set(memoryKey(String(conversationId), String(turnId)), usage);
  // Soft cap so long sessions cannot grow without bound.
  if (turnCreditsMemory.size > MAX_TURN_CREDIT_ENTRIES * 4) {
    const first = turnCreditsMemory.keys().next().value;
    if (first) turnCreditsMemory.delete(first);
  }
}

function isFlowyCloudConversation(conversation: TChatConversation | null | undefined): boolean {
  if (!conversation || conversation.type !== 'nomi') return false;
  const rawId = conversation.model?.id;
  if (!rawId) return false;
  try {
    return parseProviderId(String(rawId)) === FLOWY_BUILTIN_PROVIDER_ID;
  } catch {
    return String(rawId) === FLOWY_BUILTIN_PROVIDER_ID || String(rawId) === 'flowy-cloud';
  }
}

function pruneTurnCreditMap(
  map: Record<string, TurnCreditUsageData>,
  keepId: string
): Record<string, TurnCreditUsageData> {
  const entries = Object.entries(map);
  if (entries.length <= MAX_TURN_CREDIT_ENTRIES) return map;
  const next: Record<string, TurnCreditUsageData> = {};
  const start = Math.max(0, entries.length - MAX_TURN_CREDIT_ENTRIES + 1);
  for (const [key, value] of entries.slice(start)) {
    next[key] = value;
  }
  if (map[keepId]) next[keepId] = map[keepId];
  return next;
}

function publishTurnCredits(params: {
  conversation_id: ConversationId;
  turn_id: MessageId;
  usage: TurnCreditUsageData;
  conversation: TChatConversation;
  /** Extra lookup keys (e.g. provisional user msg id while UI still seeds root=msg_id). */
  alias_turn_ids?: Array<MessageId | string | null | undefined>;
}): void {
  const { conversation_id, turn_id, usage, conversation } = params;
  const aliasIds = (params.alias_turn_ids ?? [])
    .map((id) => (id == null ? '' : String(id).trim()))
    .filter((id) => id.length > 0 && id !== String(turn_id) && id !== usage.turnId);

  rememberTurnCredits(conversation_id, turn_id, usage);
  // Also index under server-echoed turnId when it differs from the wire id.
  if (usage.turnId && usage.turnId !== String(turn_id)) {
    rememberTurnCredits(conversation_id, usage.turnId, usage);
  }
  for (const alias of aliasIds) {
    rememberTurnCredits(conversation_id, alias, usage);
  }

  emitter.emit('nomi.turn_credits.updated', {
    conversation_id,
    turn_id,
    usage,
  });
  for (const alias of aliasIds) {
    emitter.emit('nomi.turn_credits.updated', {
      conversation_id,
      turn_id: alias as MessageId,
      usage,
    });
  }

  // Optimistic SWR update so remounted MessageText can read persisted credits
  // immediately — do not wait for the PATCH round-trip (first-turn race).
  const existing =
    (conversation.type === 'nomi' && conversation.extra?.turn_credit_usage) || {};
  const wireId = String(turn_id);
  const keyed: Record<string, TurnCreditUsageData> = {
    ...existing,
    [usage.turnId]: usage,
  };
  // MessageText looks up by message.turn_id (root) — keep both keys when the
  // server echo differs from the wire billing id.
  if (wireId && wireId !== usage.turnId) {
    keyed[wireId] = usage;
  }
  for (const alias of aliasIds) {
    keyed[alias] = usage;
  }
  const merged = pruneTurnCreditMap(keyed, usage.turnId || wireId);
  void mutate<TChatConversation>(
    `conversation/${conversation_id}`,
    (current) => {
      const base = current ?? conversation;
      if (!base || base.type !== 'nomi') return current;
      return {
        ...base,
        extra: {
          ...base.extra,
          turn_credit_usage: merged,
        },
      };
    },
    { revalidate: false }
  );

  void ipcBridge.conversation.update
    .invoke({
      conversation_id,
      updates: {
        extra: { turn_credit_usage: merged } as TChatConversation['extra'],
      },
      merge_extra: true,
    })
    .then((ok) => {
      if (ok) {
        void refreshConversationCache(conversation_id);
      }
    })
    .catch((error) => {
      console.warn('[nomi] failed to persist turn_credit_usage', error);
    });
}

/**
 * Query server-side credits for `turnId` when the conversation uses Flowy cloud.
 * Best-effort: failures are logged and never surface to the user as turn errors.
 *
 * Dedupes concurrent callers (finish + turn.completed) for the same turn.
 */
export async function fetchAndPersistTurnCredits(params: {
  conversation_id: ConversationId;
  turn_id: MessageId;
  /** Optional short delay so the last model call can finish billing write. */
  delayMs?: number;
  /** Also index the result under these ids (provisional user msg id, etc.). */
  alias_turn_ids?: Array<MessageId | string | null | undefined>;
  /** Internal: late retries after empty first-turn billing lag. */
  retryAttempt?: number;
}): Promise<TurnCreditUsageData | null> {
  const turnId = String(params.turn_id).trim();
  if (!turnId || turnId.length > 64) {
    return null;
  }

  const key = memoryKey(String(params.conversation_id), turnId);
  const existingInflight = inflightFetches.get(key);
  if (existingInflight) {
    return existingInflight;
  }

  const cached = peekTurnCredits(params.conversation_id, turnId);
  // A positive prior result is authoritative enough to skip; empty results may
  // be first-turn billing lag and are allowed to refresh.
  if (cached && (cached.callCount > 0 || cached.creditsConsumed > 0)) {
    return cached;
  }

  const run = (async (): Promise<TurnCreditUsageData | null> => {
    const conversation = await getConversationOrNull(params.conversation_id);
    if (!isFlowyCloudConversation(conversation)) {
      return null;
    }

    if (params.delayMs && params.delayMs > 0) {
      await new Promise((resolve) => setTimeout(resolve, params.delayMs));
    }

    const queryOnce = async () => ipcBridge.media.getCreditsUsageByTurn.invoke({ turnId });

    try {
      let result = await queryOnce();
      if (!result?.authenticated) {
        return null;
      }

      // First turn of a new session often needs longer for billing to land.
      const backoffMs = [1200, 2000];
      for (const wait of backoffMs) {
        if ((result.callCount ?? 0) > 0 || (result.creditsConsumed ?? 0) > 0) break;
        await new Promise((resolve) => setTimeout(resolve, wait));
        const retry = await queryOnce();
        if (retry?.authenticated) {
          result = retry;
        }
      }

      const usage: TurnCreditUsageData = {
        turnId: result.turnId || turnId,
        creditsConsumed: result.creditsConsumed ?? 0,
        callCount: result.callCount ?? 0,
        calls: (result.calls ?? []).map((call) => ({
          modelName: call.modelName,
          creditConsumed: call.creditConsumed,
          callStatus: call.callStatus,
        })),
      };

      // Do not persist/publish empty usage as a terminal first-turn result —
      // billing lag after Guid session ensure can still land later.
      if ((usage.callCount ?? 0) === 0 && (usage.creditsConsumed ?? 0) === 0) {
        const attempt = params.retryAttempt ?? 0;
        if (attempt < 2) {
          const lateDelayMs = attempt === 0 ? 5000 : 10000;
          setTimeout(() => {
            void fetchAndPersistTurnCredits({
              conversation_id: params.conversation_id,
              turn_id: params.turn_id,
              alias_turn_ids: params.alias_turn_ids,
              retryAttempt: attempt + 1,
            });
          }, lateDelayMs);
        }
        return usage;
      }

      if (conversation) {
        publishTurnCredits({
          conversation_id: params.conversation_id,
          turn_id: params.turn_id,
          usage,
          conversation,
          alias_turn_ids: params.alias_turn_ids,
        });
      }

      return usage;
    } catch (error) {
      console.warn('[nomi] failed to fetch turn credit usage', error);
      return null;
    }
  })();

  inflightFetches.set(key, run);
  try {
    return await run;
  } finally {
    inflightFetches.delete(key);
  }
}
