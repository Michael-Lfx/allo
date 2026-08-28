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
/** Late queries after finish: knowledge write-back / title / review bill after the Agent Run. */
export const TURN_CREDIT_LATE_REFRESH_MS = [2500, 8000, 22000] as const;

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

export function isPositiveTurnCreditUsage(usage: TurnCreditUsageData | null | undefined): boolean {
  return usage != null && ((usage.callCount ?? 0) > 0 || (usage.creditsConsumed ?? 0) > 0);
}

export function shouldReuseCachedTurnCredits(
  cached: TurnCreditUsageData | null,
  force: boolean
): boolean {
  return !force && isPositiveTurnCreditUsage(cached);
}

/** Keep the richer of two snapshots so a late write-back cannot be overwritten by a stale poll. */
export function pickRicherTurnCredits(
  previous: TurnCreditUsageData | null | undefined,
  next: TurnCreditUsageData
): TurnCreditUsageData {
  if (!previous || !isPositiveTurnCreditUsage(previous)) return next;
  if (!isPositiveTurnCreditUsage(next)) return previous;
  const previousCalls = previous.calls?.length ?? 0;
  const nextCalls = next.calls?.length ?? 0;
  if (
    next.creditsConsumed > previous.creditsConsumed ||
    next.callCount > previous.callCount ||
    nextCalls > previousCalls
  ) {
    return next;
  }
  return previous;
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

function scheduleLateTurnCreditRefresh(params: {
  conversation_id: ConversationId;
  turn_id: MessageId;
  alias_turn_ids?: Array<MessageId | string | null | undefined>;
  force?: boolean;
  retryAttempt?: number;
}): void {
  const attempt = params.retryAttempt ?? 0;
  // Initial finish query starts one follow-up chain. A write-back `force`
  // refetch (attempt 0) must not open a second chain; chain steps pass attempt > 0.
  if (attempt === 0 && params.force) return;
  if (attempt >= TURN_CREDIT_LATE_REFRESH_MS.length) return;
  const delayMs = TURN_CREDIT_LATE_REFRESH_MS[attempt];
  setTimeout(() => {
    void fetchAndPersistTurnCredits({
      conversation_id: params.conversation_id,
      turn_id: params.turn_id,
      alias_turn_ids: params.alias_turn_ids,
      force: true,
      retryAttempt: attempt + 1,
    });
  }, delayMs);
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
  /** Bypass the positive-cache skip (write-back settled / late billing). */
  force?: boolean;
  /** Internal: late retries after empty first-turn billing lag. */
  retryAttempt?: number;
}): Promise<TurnCreditUsageData | null> {
  const turnId = String(params.turn_id).trim();
  if (!turnId || turnId.length > 64) {
    return null;
  }

  const key = memoryKey(String(params.conversation_id), turnId);
  const existingInflight = inflightFetches.get(key);
  if (existingInflight && !params.force) {
    return existingInflight;
  }

  const cached = peekTurnCredits(params.conversation_id, turnId);
  if (shouldReuseCachedTurnCredits(cached, params.force === true)) {
    return cached;
  }

  const run = (async (): Promise<TurnCreditUsageData | null> => {
    if (existingInflight) {
      await existingInflight.catch(() => null);
    }

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

      if (!params.force) {
        const backoffMs = [1200, 2000];
        for (const wait of backoffMs) {
          if ((result.callCount ?? 0) > 0 || (result.creditsConsumed ?? 0) > 0) break;
          await new Promise((resolve) => setTimeout(resolve, wait));
          const retry = await queryOnce();
          if (retry?.authenticated) {
            result = retry;
          }
        }
      }

      const fetched: TurnCreditUsageData = {
        turnId: result.turnId || turnId,
        creditsConsumed: result.creditsConsumed ?? 0,
        callCount: result.callCount ?? 0,
        calls: (result.calls ?? []).map((call) => ({
          modelName: call.modelName,
          creditConsumed: call.creditConsumed,
          callStatus: call.callStatus,
        })),
      };
      const prior = peekTurnCredits(params.conversation_id, turnId);
      const usage = pickRicherTurnCredits(prior, fetched);

      emitter.emit('nomi.credits.balance.refresh');
      scheduleLateTurnCreditRefresh(params);

      if (!isPositiveTurnCreditUsage(usage) || !conversation) {
        return usage;
      }

      const changed =
        !prior ||
        usage.creditsConsumed !== prior.creditsConsumed ||
        usage.callCount !== prior.callCount ||
        (usage.calls?.length ?? 0) !== (prior.calls?.length ?? 0);
      if (changed) {
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
    if (inflightFetches.get(key) === run) {
      inflightFetches.delete(key);
    }
  }
}
