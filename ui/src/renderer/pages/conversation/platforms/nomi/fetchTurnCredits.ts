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
/** Extra polls on the first fetch so a 50-call coding turn is not frozen at the first positive snapshot. */
export const TURN_CREDIT_SETTLE_MS = [1500, 2500, 4000, 8000] as const;

/** Survives MessageText remounts (first-turn list reconcile races the emit). */
const turnCreditsMemory = new Map<string, TurnCreditUsageData>();
const inflightFetches = new Map<string, Promise<TurnCreditUsageData | null>>();
/** PATCH replaces the whole usage map, so different turns share a write lane. */
const inflightWrites = new Map<string, Promise<TurnCreditUsageData>>();

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

export function isFlowyCloudConversation(conversation: { type: string; model?: { id?: string }; extra?: unknown } | null | undefined): boolean {
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

/** Prefer call-sum when Flowy's aggregate lags the per-call list. */
export function normalizeTurnCreditUsage(usage: TurnCreditUsageData): TurnCreditUsageData {
  const calls = usage.calls ?? [];
  const callSum = calls.reduce((sum, call) => sum + (Number(call.creditConsumed) || 0), 0);
  const creditsConsumed = Math.max(Number(usage.creditsConsumed) || 0, callSum);
  const callCount = Math.max(Number(usage.callCount) || 0, calls.length);
  return { ...usage, creditsConsumed, callCount, calls };
}

function turnCreditFingerprint(usage: TurnCreditUsageData): string {
  return `${usage.creditsConsumed}|${usage.callCount}|${usage.calls?.length ?? 0}`;
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

async function publishTurnCredits(params: {
  conversation_id: ConversationId;
  turn_id: MessageId;
  usage: TurnCreditUsageData;
  /** Extra lookup keys (e.g. provisional user msg id while UI still seeds root=msg_id). */
  alias_turn_ids?: Array<MessageId | string | null | undefined>;
}): Promise<TurnCreditUsageData> {
  const { conversation_id, turn_id } = params;
  const writeKey = String(conversation_id);
  const previousWrite = inflightWrites.get(writeKey);
  const write = (async () => {
    await previousWrite?.catch(() => undefined);
    // Read only after earlier writes have committed, never from the query's
    // pre-network snapshot. Ordinary and attributed callers use this same lane.
    const conversation = await getConversationOrNull(conversation_id);
    if (!conversation || conversation.type !== 'nomi') {
      throw new Error('turn_credit_conversation_unavailable');
    }
    const existing = conversation.extra?.turn_credit_usage ?? {};
    const usage = pickRicherTurnCredits(
      existing[String(turn_id)] ?? existing[params.usage.turnId],
      params.usage
    );
    const aliasIds = (params.alias_turn_ids ?? [])
      .map((id) => (id == null ? '' : String(id).trim()))
      .filter((id) => id.length > 0 && id !== String(turn_id) && id !== usage.turnId);

    const wireId = String(turn_id);
    const mergeUsage = (base: Record<string, TurnCreditUsageData>) => {
      const keyed = { ...base };
      for (const key of new Set([usage.turnId, wireId, ...aliasIds])) {
        if (key) keyed[key] = pickRicherTurnCredits(keyed[key], usage);
      }
      return pruneTurnCreditMap(keyed, usage.turnId || wireId);
    };
    const merged = mergeUsage(existing);
    // Keep first-turn UI responsive while also merging with the latest SWR value.
    await mutate<TChatConversation>(
      `conversation/${conversation_id}`,
      (current) => {
        const base = current ?? conversation;
        if (!base || base.type !== 'nomi') return current;
        return {
          ...base,
          extra: {
            ...base.extra,
            turn_credit_usage: mergeUsage(base.extra?.turn_credit_usage ?? {}),
          },
        };
      },
      { revalidate: false }
    );

    const saved = await ipcBridge.conversation.update.invoke({
      conversation_id,
      updates: {
        extra: { turn_credit_usage: merged } as TChatConversation['extra'],
      },
    });
    if (!saved) throw new Error('turn_credit_persist_failed');
    // A failed PATCH must not leave a positive cache entry that prevents retry.
    for (const key of new Set([wireId, usage.turnId, ...aliasIds])) {
      if (key) rememberTurnCredits(conversation_id, key, usage);
    }
    emitter.emit('nomi.turn_credits.updated', { conversation_id, turn_id, usage });
    for (const alias of aliasIds) {
      emitter.emit('nomi.turn_credits.updated', { conversation_id, turn_id: alias as MessageId, usage });
    }
    try {
      await refreshConversationCache(conversation_id);
    } catch (error) {
      console.warn('[nomi] failed to refresh persisted turn_credit_usage', error);
    }
    return usage;
  })();
  inflightWrites.set(writeKey, write);
  try {
    return await write;
  } finally {
    if (inflightWrites.get(writeKey) === write) inflightWrites.delete(writeKey);
  }
}

export interface TurnCreditQueryParams {
  conversation_id: ConversationId;
  turn_id: MessageId;
  /** Optional short delay so the last model call can finish billing write. */
  delayMs?: number;
  /** Also index the result under these ids (provisional user msg id, etc.). */
  alias_turn_ids?: Array<MessageId | string | null | undefined>;
  /** Bypass the positive-cache skip (write-back settled / late billing). */
  force?: boolean;
  /**
   * Agent Run terminal: ignore a positive in-memory snapshot (often the first
   * billed call captured while tools/goal continuations were still running)
   * and run the settle loop. Unlike `force`, this still schedules late refresh.
   */
  settle?: boolean;
  /** Internal: late retries after empty first-turn billing lag. */
  retryAttempt?: number;
}

function scheduleLateTurnCreditRefresh(
  params: TurnCreditQueryParams,
  requireCloudProvider: boolean
): void {
  const attempt = params.retryAttempt ?? 0;
  // Initial finish query starts one follow-up chain. A write-back `force`
  // refetch (attempt 0) must not open a second chain; chain steps pass attempt > 0.
  if (attempt === 0 && params.force) return;
  if (attempt >= TURN_CREDIT_LATE_REFRESH_MS.length) return;
  const delayMs = TURN_CREDIT_LATE_REFRESH_MS[attempt];
  setTimeout(() => {
    void runTurnCreditQuery(
      {
        conversation_id: params.conversation_id,
        turn_id: params.turn_id,
        alias_turn_ids: params.alias_turn_ids,
        force: true,
        retryAttempt: attempt + 1,
      },
      requireCloudProvider
    );
  }, delayMs);
}

/**
 * Query server-side credits for `turnId`.
 *
 * `requireCloudProvider` is the ordinary conversation gate. The attributed
 * entry below passes `false` only after its caller has already established
 * that this exact turn was billed through Flowy cloud (for example by a
 * server-issued durable turn identity). Query, cache and persistence stay
 * shared here; neither mode knows about a caller-specific state machine.
 *
 * Dedupes concurrent callers (finish + turn.completed) for the same turn.
 */
async function runTurnCreditQuery(
  params: TurnCreditQueryParams,
  requireCloudProvider: boolean
): Promise<TurnCreditUsageData | null> {
  const turnId = String(params.turn_id).trim();
  if (!turnId || turnId.length > 64) {
    return null;
  }

  const key = memoryKey(String(params.conversation_id), turnId);
  const existingInflight = inflightFetches.get(key);
  if (existingInflight && !params.force && !params.settle) {
    return existingInflight;
  }

  const cached = peekTurnCredits(params.conversation_id, turnId);
  if (shouldReuseCachedTurnCredits(cached, params.force === true || params.settle === true)) {
    return cached;
  }

  // Resolve eligibility before publishing an inflight entry. A rejected
  // ordinary gate is not a billing answer and must never be reused by an
  // attributed caller racing for the same turn.
  let conversation;
  try {
    conversation = await getConversationOrNull(params.conversation_id);
  } catch (error) {
    console.warn('[nomi] failed to read conversation for turn credit usage', error);
    return null;
  }
  if (!conversation) {
    return null;
  }
  if (requireCloudProvider && !isFlowyCloudConversation(conversation)) {
    return null;
  }

  // Two eligible callers may have crossed the async eligibility read above;
  // the second one reuses the first one's query instead of starting another.
  const concurrentInflight = inflightFetches.get(key);
  if (concurrentInflight && !params.force && !params.settle) {
    return concurrentInflight;
  }

  const run = (async (): Promise<TurnCreditUsageData | null> => {
    if (concurrentInflight) {
      await concurrentInflight.catch(() => null);
    }

    if (params.delayMs && params.delayMs > 0) {
      await new Promise((resolve) => setTimeout(resolve, params.delayMs));
    }

    const queryOnce = async () => ipcBridge.media.getCreditsUsageByTurn.invoke({ turnId });

    const toUsage = (result: {
      turnId?: string;
      creditsConsumed?: number;
      callCount?: number;
      calls?: Array<{ modelName: string; creditConsumed: number; callStatus?: string }>;
    }): TurnCreditUsageData =>
      normalizeTurnCreditUsage({
        turnId: result.turnId || turnId,
        creditsConsumed: result.creditsConsumed ?? 0,
        callCount: result.callCount ?? 0,
        calls: (result.calls ?? []).map((call) => ({
          modelName: call.modelName,
          creditConsumed: call.creditConsumed,
          callStatus: call.callStatus,
        })),
      });

    try {
      let result = await queryOnce();
      if (!result?.authenticated) {
        return null;
      }

      let usage = toUsage(result);
      if (!params.force) {
        let previous = turnCreditFingerprint(usage);
        for (const wait of TURN_CREDIT_SETTLE_MS) {
          await new Promise((resolve) => setTimeout(resolve, wait));
          const retry = await queryOnce();
          if (!retry?.authenticated) break;
          usage = pickRicherTurnCredits(usage, toUsage(retry));
          const next = turnCreditFingerprint(usage);
          if (next === previous && isPositiveTurnCreditUsage(usage)) break;
          previous = next;
        }
      }

      const prior = peekTurnCredits(params.conversation_id, turnId);
      usage = pickRicherTurnCredits(prior, usage);

      emitter.emit('nomi.credits.balance.refresh');
      scheduleLateTurnCreditRefresh(params, requireCloudProvider);

      if (!isPositiveTurnCreditUsage(usage)) {
        return usage;
      }

      const changed =
        !prior ||
        usage.creditsConsumed !== prior.creditsConsumed ||
        usage.callCount !== prior.callCount ||
        (usage.calls?.length ?? 0) !== (prior.calls?.length ?? 0);
      if (changed) {
        usage = await publishTurnCredits({
          conversation_id: params.conversation_id,
          turn_id: params.turn_id,
          usage,
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

/**
 * Ordinary conversation entry. Keeps the original cloud-provider gate so local
 * and third-party task models never issue an extra usage query.
 */
export async function fetchAndPersistTurnCredits(
  params: TurnCreditQueryParams
): Promise<TurnCreditUsageData | null> {
  return runTurnCreditQuery(params, true);
}

/**
 * Entry for a turn whose Flowy-cloud attribution was already established by
 * its server-issued durable identity. Shares the same query, cache, dedupe and
 * persistence core as the ordinary entry; it does not add a second fetch path.
 */
export async function fetchAndPersistAttributedTurnCredits(
  params: TurnCreditQueryParams
): Promise<TurnCreditUsageData | null> {
  return runTurnCreditQuery(params, false);
}
