/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useEffect, useMemo, useRef, useState } from 'react';
import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';
import type { TMessage } from '@common/chat/chatLib';
import type { ConversationId, MessageId } from '@common/types/ids';
import { fetchAndPersistAttributedTurnCredits, TURN_CREDIT_LATE_REFRESH_MS } from '../conversation/platforms/nomi/fetchTurnCredits';

export interface VoiceBilledTurn {
  /** Acoustic turn id used only to correlate the snapshot window. */
  turnId: string;
  /** Durable user message id; server billing attribution uses this identity. */
  userMessageId: string;
}

const MAX_ATTEMPTED_TURNS = 128;
const MAX_HISTORY_TURNS = 12;

/**
 * Only a turn whose durable user row is already projected may trigger a credit
 * query. The binding must also belong to the conversation currently on screen,
 * so browsing another conversation can never query the active call's turns.
 */
export function collectVoiceBilledTurns(
  snapshot: VoiceSnapshot | null,
  conversationId: ConversationId | string
): VoiceBilledTurn[] {
  if (!snapshot?.binding || snapshot.binding.conversation_id !== conversationId) return [];
  if (!snapshot.session_id || snapshot.binding.session_id !== snapshot.session_id) return [];
  const projected = new Set(snapshot.projected_turn_ids ?? []);
  const seen = new Set<string>();
  const turns: VoiceBilledTurn[] = [];
  for (const turn of snapshot.conversation ?? []) {
    // Match liveVoiceTurn: a turn from another generation is not owned by the
    // current binding, even when the conversation id happens to match.
    if (!turn.session_id || turn.session_id !== snapshot.session_id) continue;
    const userMessageId = turn.user_message_id;
    if (!userMessageId || seen.has(userMessageId)) continue;
    if (!projected.has(turn.turn_id) && !turn.assistant_message_id) continue;
    seen.add(userMessageId);
    turns.push({ turnId: turn.turn_id, userMessageId });
  }
  return turns;
}

/**
 * Bounded backfill from already-loaded durable history. Only assistant text
 * rows with a server-assigned message id and an explicit turn id qualify;
 * pending/live rows without durable identity are never guessed. Cross-
 * conversation rows and task-agent output are excluded.
 */
export function collectVoiceHistoryBilledTurns(
  messages: readonly TMessage[] | undefined,
  conversationId: ConversationId | string
): VoiceBilledTurn[] {
  if (!messages?.length) return [];
  const turns: VoiceBilledTurn[] = [];
  const seen = new Set<string>();
  for (
    let index = messages.length - 1;
    index >= 0 && turns.length < MAX_HISTORY_TURNS;
    index -= 1
  ) {
    const message = messages[index];
    if (message.conversation_id !== conversationId) continue;
    if (message.position !== 'left' || message.type !== 'text') continue;
    if (message.hidden) continue;
    if (!message.message_id || !message.turn_id) continue;
    if (message.status != null && message.status !== 'finish') continue;
    if (
      message.content &&
      typeof message.content === 'object' &&
      (message.content as Record<string, unknown>).agentMessage === true
    ) {
      continue;
    }
    const id = String(message.turn_id);
    if (seen.has(id)) continue;
    seen.add(id);
    turns.push({ turnId: id, userMessageId: id });
  }
  return turns;
}

export interface VoiceCreditAttempt {
  attempts: number;
  complete: boolean;
  pending: Promise<void> | null;
  retryAt: number;
}

/** Successful turns stay deduped; failures have a bounded, timed retry budget. */
export function scheduleVoiceTurnCredits(
  turns: readonly VoiceBilledTurn[],
  conversationId: ConversationId | string,
  attempted: Map<string, VoiceCreditAttempt>,
  query: (turn: VoiceBilledTurn) => unknown,
  now = Date.now()
): number {
  let scheduled = 0;
  for (const turn of turns) {
    const key = `${conversationId}\0${turn.userMessageId}`;
    const previous = attempted.get(key);
    if (previous && (previous.complete || previous.pending || now < previous.retryAt)) continue;
    const entry: VoiceCreditAttempt = {
      attempts: (previous?.attempts ?? 0) + 1,
      complete: false,
      pending: null,
      retryAt: Infinity,
    };
    attempted.set(key, entry);
    scheduled += 1;
    const settle = (complete: boolean) => {
      entry.complete = complete;
      entry.pending = null;
      const delay = TURN_CREDIT_LATE_REFRESH_MS[entry.attempts - 1];
      entry.retryAt = !complete && delay != null ? Date.now() + delay : Infinity;
    };
    let result: unknown;
    try {
      result = query(turn);
    } catch (error) {
      result = Promise.reject(error);
    }
    entry.pending = Promise.resolve(result).then(
      (value) => settle(value !== null),
      () => settle(false)
    );
  }
  if (attempted.size > MAX_ATTEMPTED_TURNS) {
    for (const [key, entry] of attempted) {
      if (!entry.pending) attempted.delete(key);
      if (attempted.size <= MAX_ATTEMPTED_TURNS) break;
    }
  }
  return scheduled;
}

/**
 * Voice-owned timing adapter over the shared turn-credit core. Ordinary
 * conversations never mount this hook, and RMS-only snapshot updates cannot
 * schedule another query because the durable identity set is deduped.
 */
export function useVoiceTurnCredits(
  conversationId: ConversationId | string,
  snapshot: VoiceSnapshot | null,
  historyMessages?: readonly TMessage[],
  enabled = true
): readonly VoiceBilledTurn[] {
  const attemptedRef = useRef(new Map<string, VoiceCreditAttempt>());
  const [recoverySequence, setRecoverySequence] = useState(0);
  const historyTurns = useMemo(
    () => collectVoiceHistoryBilledTurns(historyMessages, conversationId),
    [conversationId, historyMessages]
  );
  const turns = useMemo(() => {
    if (!enabled) return [];
    const merged = [...collectVoiceBilledTurns(snapshot, conversationId), ...historyTurns];
    const seen = new Set<string>();
    return merged.filter((turn) => {
      if (seen.has(turn.userMessageId)) return false;
      seen.add(turn.userMessageId);
      return true;
    });
  }, [conversationId, enabled, historyTurns, snapshot]);

  useEffect(() => {
    if (!enabled || typeof window === 'undefined') return;
    const recover = () => {
      for (const [key, entry] of attemptedRef.current) {
        if (key.startsWith(`${conversationId}\0`) && !entry.complete && !entry.pending) {
          attemptedRef.current.delete(key);
        }
      }
      setRecoverySequence((value) => value + 1);
    };
    window.addEventListener('online', recover);
    window.addEventListener('focus', recover);
    return () => {
      window.removeEventListener('online', recover);
      window.removeEventListener('focus', recover);
    };
  }, [conversationId, enabled]);

  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const run = () => {
      if (disposed) return;
      if (timer != null) clearTimeout(timer);
      scheduleVoiceTurnCredits(turns, conversationId, attemptedRef.current, (turn) =>
        fetchAndPersistAttributedTurnCredits({
          conversation_id: conversationId as ConversationId,
          turn_id: turn.userMessageId as MessageId,
          delayMs: 400,
        })
      );
      const pending: Promise<void>[] = [];
      let retryAt = Infinity;
      for (const turn of turns) {
        const entry = attemptedRef.current.get(`${conversationId}\0${turn.userMessageId}`);
        if (entry?.pending) pending.push(entry.pending);
        else if (entry && !entry.complete) retryAt = Math.min(retryAt, entry.retryAt);
      }
      // Read pending promises even when they came from a previous RMS render.
      // Its effect may have disposed while the request was still in flight.
      if (pending.length) void Promise.all(pending).then(run);
      if (Number.isFinite(retryAt)) timer = setTimeout(run, Math.max(0, retryAt - Date.now()));
    };
    run();
    return () => {
      disposed = true;
      if (timer != null) clearTimeout(timer);
    };
  }, [conversationId, enabled, turns, recoverySequence]);
  return turns;
}
