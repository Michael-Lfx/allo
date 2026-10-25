/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import * as bunTest from 'bun:test';
import type { VoiceCreditAttempt } from '../../../voiceSession/useVoiceTurnCredits';

const { beforeEach, describe, expect, test } = bunTest;
const mock = (bunTest as unknown as { mock: { module: (specifier: string, factory: () => unknown) => void } }).mock;

type ConversationStub = {
  id: string;
  type: 'nomi';
  model?: { id?: string; use_model?: string };
  extra?: Record<string, unknown>;
};

const FLOWY_PROVIDER = '0190f5fe-7c00-7a00-8000-000000000201';
const LOCAL_PROVIDER = '0190f5fe-7c00-7a00-8000-000000000901';
const THIRD_PARTY_PROVIDER = '0190f5fe-7c00-7a00-8000-000000000902';

const conversations = new Map<string, ConversationStub>();
const queryCalls: string[] = [];
const events: Array<{ name: string; payload: unknown }> = [];
const swrConversations = new Map<string, ConversationStub>();
let authenticated = true;
let rejectWrite = false;
let activeWrites = 0;
let maxActiveWrites = 0;
const queryDelays = new Map<string, number>();
const echoedTurnIds = new Map<string, string>();

mock.module('@/common', () => ({
  ipcBridge: {
    media: {
      getCreditsUsageByTurn: {
        invoke: async ({ turnId }: { turnId: string }) => {
          queryCalls.push(turnId);
          if (queryDelays.has(turnId)) await new Promise((resolve) => setTimeout(resolve, queryDelays.get(turnId)));
          return {
            authenticated,
            turnId: echoedTurnIds.get(turnId) ?? turnId,
            creditsConsumed: 120,
            callCount: 1,
            calls: [{ modelName: 'quick-reply', creditConsumed: 120, callStatus: 'completed' }],
          };
        },
      },
    },
    conversation: {
      update: { invoke: async ({ conversation_id, updates }: { conversation_id: string; updates: { extra: Record<string, unknown> } }) => {
        activeWrites += 1;
        maxActiveWrites = Math.max(maxActiveWrites, activeWrites);
        try {
          await new Promise((resolve) => setTimeout(resolve, 2));
          if (rejectWrite) return false;
          const current = conversations.get(conversation_id)!;
          // The real PATCH merges extra at the top level, not nested usage maps.
          conversations.set(conversation_id, { ...current, extra: { ...current.extra, ...structuredClone(updates.extra) } });
          return true;
        } finally { activeWrites -= 1; }
      } },
    },
  },
}));

mock.module('@/renderer/pages/conversation/utils/conversationCache', () => ({
  getConversationOrNull: async (id: string) => structuredClone(conversations.get(String(id)) ?? null),
  refreshConversationCache: async (id: string) => {
    const current = conversations.get(String(id));
    if (current) swrConversations.set(String(id), structuredClone(current));
  },
}));

mock.module('swr', () => ({
  mutate: async (key: string, updater: (current?: ConversationStub) => ConversationStub) => {
    const id = key.slice('conversation/'.length);
    const next = updater(swrConversations.get(id));
    if (next) swrConversations.set(id, next);
  },
}));

mock.module('@/renderer/utils/emitter', () => ({
  emitter: {
    emit: (name: string, payload: unknown) => events.push({ name, payload }),
    on: () => undefined,
    off: () => undefined,
  },
}));

const {
  fetchAndPersistTurnCredits,
  fetchAndPersistAttributedTurnCredits,
  isFlowyCloudConversation,
  peekTurnCredits,
} = await import('./fetchTurnCredits');
const {
  collectVoiceBilledTurns,
  collectVoiceHistoryBilledTurns,
  scheduleVoiceTurnCredits,
} = await import('../../../voiceSession/useVoiceTurnCredits');

const conversation = (
  id: string,
  providerId: string,
  extra: Record<string, unknown> = {}
): ConversationStub => ({
  id,
  type: 'nomi',
  model: { id: providerId, use_model: 'task-model' },
  extra,
});

beforeEach(() => {
  conversations.clear();
  queryCalls.length = 0;
  events.length = 0;
  swrConversations.clear();
  authenticated = true;
  rejectWrite = false;
  activeWrites = 0;
  maxActiveWrites = 0;
  queryDelays.clear();
  echoedTurnIds.clear();
});

describe('durable credit writeback integrity', () => {
  test('independent review: parallel history turns preserve the whole persisted map', async () => {
    const id = 'parallel-history-writeback';
    conversations.set(id, conversation(id, LOCAL_PROVIDER, { voice: true, workspace: 'D:/project' }));
    const turnIds = Array.from({ length: 12 }, (_, index) => `parallel-turn-${index}`);
    turnIds.forEach((id, index) => queryDelays.set(id, (12 - index) * 2));
    await Promise.all(turnIds.map((turn_id) => fetchAndPersistAttributedTurnCredits({
      conversation_id: id as never, turn_id: turn_id as never, force: true,
    })));
    expect(Object.keys(conversations.get(id)!.extra!.turn_credit_usage as object).sort()).toEqual([...turnIds].sort());
    expect(Object.keys(swrConversations.get(id)!.extra!.turn_credit_usage as object).sort()).toEqual([...turnIds].sort());
    expect(conversations.get(id)!.extra!.workspace).toBe('D:/project');
    expect(maxActiveWrites).toBe(1);
  });

  test('ordinary and attributed callers share writes without blocking another conversation', async () => {
    const id = 'mixed-credit-writes';
    const otherId = 'other-credit-writes';
    conversations.set(id, conversation(id, FLOWY_PROVIDER));
    conversations.set(otherId, conversation(otherId, LOCAL_PROVIDER));
    await Promise.all([
      fetchAndPersistTurnCredits({ conversation_id: id as never, turn_id: 'mixed-ordinary' as never, force: true }),
      fetchAndPersistAttributedTurnCredits({ conversation_id: id as never, turn_id: 'mixed-attributed' as never, force: true }),
      fetchAndPersistAttributedTurnCredits({ conversation_id: otherId as never, turn_id: 'other-turn' as never, force: true }),
    ]);
    expect(Object.keys(conversations.get(id)!.extra!.turn_credit_usage as object).sort()).toEqual(['mixed-attributed', 'mixed-ordinary']);
    expect(Object.keys(conversations.get(otherId)!.extra!.turn_credit_usage as object)).toEqual(['other-turn']);
    expect(maxActiveWrites).toBe(2);
  });

  test('writeback preserves aliases and bounds the persisted map after reloading history', async () => {
    const id = 'bounded-credit-writeback';
    const oldUsage = Object.fromEntries(Array.from({ length: 40 }, (_, index) => [
      `old-${index}`, { turnId: `old-${index}`, creditsConsumed: 1, callCount: 1, calls: [] },
    ]));
    conversations.set(id, conversation(id, LOCAL_PROVIDER, { turn_credit_usage: oldUsage, session_mode: 'yolo' }));
    echoedTurnIds.set('wire-turn', 'echo-turn');
    await fetchAndPersistAttributedTurnCredits({
      conversation_id: id as never, turn_id: 'wire-turn' as never, alias_turn_ids: ['alias-turn'], force: true,
    });
    const reloaded = structuredClone(conversations.get(id)!);
    const usage = reloaded.extra!.turn_credit_usage as Record<string, { creditsConsumed: number }>;
    expect(Object.keys(usage).length).toBeLessThanOrEqual(40);
    for (const key of ['wire-turn', 'echo-turn', 'alias-turn']) {
      expect(usage[key]?.creditsConsumed).toBe(120);
      expect(peekTurnCredits(id, key)?.creditsConsumed).toBe(120);
    }
    expect(reloaded.extra!.session_mode).toBe('yolo');
  });

  test('a rejected persistence write does not poison successful-query cache or the next write', async () => {
    const id = 'recover-writeback';
    conversations.set(id, conversation(id, LOCAL_PROVIDER));
    const params = { conversation_id: id as never, turn_id: 'recover-write-turn' as never, force: true };
    rejectWrite = true;
    expect(await fetchAndPersistAttributedTurnCredits(params)).toBeNull();
    expect(peekTurnCredits(id, params.turn_id)).toBeNull();
    rejectWrite = false;
    expect((await fetchAndPersistAttributedTurnCredits({ ...params, force: false }))?.creditsConsumed).toBe(120);
    expect((conversations.get(id)!.extra!.turn_credit_usage as Record<string, unknown>)[params.turn_id]).toBeDefined();
  });

  test('independent review: failed authentication can recover for the same durable turn', async () => {
    const id = 'recover-authentication';
    conversations.set(id, conversation(id, LOCAL_PROVIDER, { voice: true }));
    const attempted = new Map<string, VoiceCreditAttempt>();
    const turns = [{ turnId: 'acoustic', userMessageId: 'recover-auth-turn' }];
    const pending: Promise<unknown>[] = [];
    const schedule = (now?: number) => scheduleVoiceTurnCredits(turns, id, attempted, (turn) => {
      const result = fetchAndPersistAttributedTurnCredits({ conversation_id: id as never, turn_id: turn.userMessageId as never, force: true });
      pending.push(result);
      return result;
    }, now);
    authenticated = false;
    schedule(); await Promise.all(pending); pending.length = 0;
    authenticated = true;
    expect(schedule()).toBe(0);
    expect(schedule(Date.now() + 2501)).toBe(1);
    await Promise.all(pending);
    expect(peekTurnCredits(id, 'recover-auth-turn')?.creditsConsumed).toBe(120);
  });
});

describe('ordinary turn credit entry stays model-gated', () => {
  test('local and third-party task models never run the voice fast-model query', async () => {
    conversations.set('conv-local', conversation('conv-local', LOCAL_PROVIDER));
    conversations.set('conv-third', conversation('conv-third', THIRD_PARTY_PROVIDER, { voice: false }));

    await fetchAndPersistTurnCredits({
      conversation_id: 'conv-local' as never,
      turn_id: 'turn-local' as never,
      force: true,
    });
    await fetchAndPersistTurnCredits({
      conversation_id: 'conv-third' as never,
      turn_id: 'turn-third' as never,
      force: true,
    });

    expect(queryCalls).toEqual([]);
    expect(peekTurnCredits('conv-local', 'turn-local')).toBeNull();
    expect(peekTurnCredits('conv-third', 'turn-third')).toBeNull();
  });

  test('a voice-marked conversation with a local task model is not cloud by the ordinary gate', async () => {
    const voice = conversation('conv-voice-local', LOCAL_PROVIDER, { voice: true });
    conversations.set('conv-voice-local', voice);

    expect(isFlowyCloudConversation(voice)).toBe(false);

    await fetchAndPersistTurnCredits({
      conversation_id: 'conv-voice-local' as never,
      turn_id: 'turn-voice-local' as never,
      force: true,
    });

    expect(queryCalls).toEqual([]);
    expect(peekTurnCredits('conv-voice-local', 'turn-voice-local')).toBeNull();
  });

  test('Flowy cloud conversations keep their original query and cache behavior', async () => {
    conversations.set('conv-cloud', conversation('conv-cloud', FLOWY_PROVIDER));

    const first = await fetchAndPersistTurnCredits({
      conversation_id: 'conv-cloud' as never,
      turn_id: 'turn-cloud' as never,
      force: true,
    });
    const second = await fetchAndPersistTurnCredits({
      conversation_id: 'conv-cloud' as never,
      turn_id: 'turn-cloud' as never,
    });

    expect(first?.creditsConsumed).toBe(120);
    expect(second?.creditsConsumed).toBe(120);
    expect(queryCalls).toEqual(['turn-cloud']);
    expect(events.filter((event) => event.name === 'nomi.turn_credits.updated')).toHaveLength(1);
  });
});

describe('attributed core is shared without teaching the ordinary gate about voice', () => {
  test('independent review: an in-flight ordinary local-model rejection cannot swallow an attributed query', async () => {
    const id = 'review-local-gate-race';
    conversations.set(id, conversation(id, LOCAL_PROVIDER, { voice: true }));
    const params = {
      conversation_id: id as never,
      turn_id: 'review-durable-fast-turn' as never,
      retryAttempt: 3,
    };
    const ordinary = fetchAndPersistTurnCredits(params);
    const attributed = fetchAndPersistAttributedTurnCredits(params);
    const [ordinaryUsage, voiceUsage] = await Promise.all([ordinary, attributed]);
    expect(ordinaryUsage).toBeNull();
    expect(voiceUsage?.creditsConsumed).toBe(120);
    expect(queryCalls).toContain(params.turn_id);
  });
  test('ordinary and attributed entries share one Conversation + turn cache', async () => {
    conversations.set('conv-shared', conversation('conv-shared', FLOWY_PROVIDER));

    const attributed = await fetchAndPersistAttributedTurnCredits({
      conversation_id: 'conv-shared' as never,
      turn_id: 'shared-turn' as never,
      force: true,
    });
    const ordinary = await fetchAndPersistTurnCredits({
      conversation_id: 'conv-shared' as never,
      turn_id: 'shared-turn' as never,
    });

    expect(attributed?.creditsConsumed).toBe(120);
    expect(ordinary?.creditsConsumed).toBe(120);
    expect(queryCalls).toEqual(['shared-turn']);
  });

  test('independent followup: durable assistant history backfills when snapshot is null and cache is empty', async () => {
    const id = 'history-voice-credits';
    conversations.set(id, conversation(id, LOCAL_PROVIDER, { voice: true }));
    const messages = [
      {
        id: 'history-local-row',
        conversation_id: id,
        message_id: 'history-assistant',
        turn_id: 'history-turn',
        position: 'left',
        type: 'text',
        status: 'finish',
        hidden: false,
        content: { content: 'persisted reply' },
      },
    ] as unknown as Parameters<typeof collectVoiceHistoryBilledTurns>[0];

    expect(collectVoiceBilledTurns(null, id)).toEqual([]);
    const turns = collectVoiceHistoryBilledTurns(messages, id);
    expect(turns).toEqual([{ turnId: 'history-turn', userMessageId: 'history-turn' }]);
    expect(peekTurnCredits(id, 'history-turn')).toBeNull();

    const attempted = new Map<string, VoiceCreditAttempt>();
    const pending: Array<Promise<unknown>> = [];
    const schedule = () =>
      scheduleVoiceTurnCredits(turns, id, attempted, (turn) => {
        pending.push(
          fetchAndPersistAttributedTurnCredits({
            conversation_id: id as never,
            turn_id: turn.userMessageId as never,
          })
        );
      });

    expect(schedule()).toBe(1);
    expect(schedule()).toBe(0);
    await Promise.all(pending);
    expect(queryCalls).toContain('history-turn');
    expect(peekTurnCredits(id, 'history-turn')?.creditsConsumed).toBe(120);
  });

  test('a voice turn can query its durable user-message id even when the task model is local', async () => {
    conversations.set(
      'conv-voice-attr',
      conversation('conv-voice-attr', LOCAL_PROVIDER, { voice: true })
    );

    const usage = await fetchAndPersistAttributedTurnCredits({
      conversation_id: 'conv-voice-attr' as never,
      turn_id: 'voice-user-message' as never,
      force: true,
    });

    expect(usage?.creditsConsumed).toBe(120);
    expect(queryCalls).toEqual(['voice-user-message']);
    expect(peekTurnCredits('conv-voice-attr', 'voice-user-message')?.creditsConsumed).toBe(120);

    await fetchAndPersistAttributedTurnCredits({
      conversation_id: 'conv-voice-attr' as never,
      turn_id: 'voice-user-message' as never,
    });
    expect(queryCalls).toEqual(['voice-user-message']);
  });
});
