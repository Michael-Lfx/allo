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

const MAX_TURN_CREDIT_ENTRIES = 40;

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
  // Keep newest-ish by insertion order approximation: drop oldest keys first,
  // always retaining the just-fetched turn.
  const next: Record<string, TurnCreditUsageData> = {};
  const start = Math.max(0, entries.length - MAX_TURN_CREDIT_ENTRIES + 1);
  for (const [key, value] of entries.slice(start)) {
    next[key] = value;
  }
  if (map[keepId]) next[keepId] = map[keepId];
  return next;
}

/**
 * Query server-side credits for `turnId` when the conversation uses Flowy cloud.
 * Best-effort: failures are logged and never surface to the user as turn errors.
 */
export async function fetchAndPersistTurnCredits(params: {
  conversation_id: ConversationId;
  turn_id: MessageId;
  /** Optional short delay so the last model call can finish billing write. */
  delayMs?: number;
}): Promise<TurnCreditUsageData | null> {
  // getConversationOrNull is async — must await or the provider gate always fails.
  const conversation = await getConversationOrNull(params.conversation_id);
  if (!isFlowyCloudConversation(conversation)) {
    return null;
  }

  const turnId = String(params.turn_id).trim();
  if (!turnId || turnId.length > 64) {
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

    // Billing write can lag the Finish event; one short retry when empty.
    if ((result.callCount ?? 0) === 0 && (result.creditsConsumed ?? 0) === 0) {
      await new Promise((resolve) => setTimeout(resolve, 1200));
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

    // callCount=0 with authenticated still means "no cloud charge recorded"
    // (local-only tools, missing header, or billing lag). Still emit so UI can
    // decide whether to hide the chip.
    emitter.emit('nomi.turn_credits.updated', {
      conversation_id: params.conversation_id,
      turn_id: params.turn_id,
      usage,
    });

    const existing =
      (conversation?.type === 'nomi' && conversation.extra?.turn_credit_usage) || {};
    const merged = pruneTurnCreditMap(
      { ...existing, [usage.turnId]: usage },
      usage.turnId
    );

    void ipcBridge.conversation.update
      .invoke({
        conversation_id: params.conversation_id,
        updates: {
          extra: { turn_credit_usage: merged } as TChatConversation['extra'],
        },
        merge_extra: true,
      })
      .then((ok) => {
        if (ok) {
          void refreshConversationCache(params.conversation_id);
        }
      })
      .catch((error) => {
        console.warn('[nomi] failed to persist turn_credit_usage', error);
      });

    return usage;
  } catch (error) {
    console.warn('[nomi] failed to fetch turn credit usage', error);
    return null;
  }
}
