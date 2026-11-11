/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';
import type { TMessage } from '@common/chat/chatLib';
import {
  collectVoiceBilledTurns,
  collectVoiceHistoryBilledTurns,
  scheduleVoiceTurnCredits,
  type VoiceCreditAttempt,
} from './useVoiceTurnCredits';

const binding = (conversationId: string, session = 'session-1') => ({
  session_id: session,
  binding_id: `binding-${session}`,
  conversation_id: conversationId,
  workspace: null,
});

const snapshot = (overrides: Partial<VoiceSnapshot> = {}): VoiceSnapshot =>
  ({
    instance_id: 'host',
    seq: 1,
    session_id: 'session-1',
    phase: 'listening',
    ended_reason: null,
    error: null,
    user_text: null,
    assistant_text: null,
    enabled: true,
    connection: 'connected',
    acoustic_ready: true,
    dialogue_ready: true,
    playback_ready: true,
    binding: binding('conv-b'),
    projected_turn_ids: ['turn-1'],
    conversation: [
      {
        turn_id: 'turn-1',
        session_id: 'session-1',
        user_message_id: 'msg-user-1',
        assistant_message_id: 'msg-assistant-1',
        user_text: 'hello',
        assistant_text: 'hi',
        model_text: null,
      },
    ],
    ...overrides,
  }) as VoiceSnapshot;

describe('voice credit coordinator', () => {
  test('failed queries cool down, stop at the retry budget, and do not retry on every RMS update', async () => {
    const attempted = new Map<string, VoiceCreditAttempt>();
    const turns = [{ turnId: 'acoustic', userMessageId: 'failed-user' }];
    let calls = 0;
    const query = async () => { calls += 1; return null; };
    expect(scheduleVoiceTurnCredits(turns, 'conv-b', attempted, query)).toBe(1);
    for (let count = 0; count < 20; count += 1) {
      expect(scheduleVoiceTurnCredits(turns, 'conv-b', attempted, query)).toBe(0);
    }
    await attempted.get('conv-b\0failed-user')!.pending;
    for (let attempt = 2; attempt <= 4; attempt += 1) {
      const entry = attempted.get('conv-b\0failed-user')!;
      expect(scheduleVoiceTurnCredits(turns, 'conv-b', attempted, query, entry.retryAt - 1)).toBe(0);
      expect(scheduleVoiceTurnCredits(turns, 'conv-b', attempted, query, entry.retryAt)).toBe(1);
      await attempted.get('conv-b\0failed-user')!.pending;
    }
    expect(calls).toBe(4);
    expect(attempted.get('conv-b\0failed-user')!.retryAt).toBe(Infinity);
    expect(scheduleVoiceTurnCredits(turns, 'conv-b', attempted, query, Date.now() + 100_000)).toBe(0);
  });

  test('a rejected query can recover once due, then remains deduped after success', async () => {
    const attempted = new Map<string, VoiceCreditAttempt>();
    const turns = [{ turnId: 'acoustic', userMessageId: 'recovered-user' }];
    scheduleVoiceTurnCredits(turns, 'conv-b', attempted, async () => { throw new Error('offline'); });
    await attempted.get('conv-b\0recovered-user')!.pending;
    const due = attempted.get('conv-b\0recovered-user')!.retryAt;
    expect(scheduleVoiceTurnCredits(turns, 'conv-b', attempted, async () => ({ creditsConsumed: 120 }), due)).toBe(1);
    await attempted.get('conv-b\0recovered-user')!.pending;
    expect(attempted.get('conv-b\0recovered-user')!.complete).toBe(true);
    expect(scheduleVoiceTurnCredits(turns, 'conv-b', attempted, () => { throw new Error('must not query twice'); }, due + 100_000)).toBe(0);
    expect(scheduleVoiceTurnCredits(turns, 'conv-other', attempted, async () => null)).toBe(1);
    await attempted.get('conv-other\0recovered-user')!.pending;
  });
  test('independent review: stale or unowned per-turn sessions cannot be attributed to the current binding', () => {
    const current = snapshot();
    for (const sessionId of ['previous-session', null, undefined]) {
      const stale = snapshot({
        conversation: current.conversation!.map((turn) => ({ ...turn, session_id: sessionId })),
      });
      expect(collectVoiceBilledTurns(stale, 'conv-b')).toEqual([]);
    }
  });
  test('queries only durable, projected user message ids for the bound conversation', () => {
    expect(collectVoiceBilledTurns(snapshot(), 'conv-b')).toEqual([
      { turnId: 'turn-1', userMessageId: 'msg-user-1' },
    ]);

    const notProjected = snapshot({
      projected_turn_ids: [],
      conversation: [
        {
          turn_id: 'turn-1',
          session_id: 'session-1',
          user_message_id: 'msg-user-1',
          assistant_message_id: null,
          user_text: 'hello',
          assistant_text: null,
          model_text: null,
        },
      ],
    });
    expect(collectVoiceBilledTurns(notProjected, 'conv-b')).toEqual([]);

    const noUserRow = snapshot({
      conversation: [
        {
          turn_id: 'turn-1',
          session_id: 'session-1',
          user_message_id: null,
          assistant_message_id: 'msg-assistant-1',
          user_text: null,
          assistant_text: 'hi',
          model_text: null,
        },
      ],
    });
    expect(collectVoiceBilledTurns(noUserRow, 'conv-b')).toEqual([]);
  });

  test('browsing another conversation never reads the active call turns', () => {
    expect(collectVoiceBilledTurns(snapshot(), 'conv-a')).toEqual([]);
    const staleSession = snapshot({ session_id: 'session-2' });
    expect(collectVoiceBilledTurns(staleSession, 'conv-b')).toEqual([]);
  });

  test('same Conversation + durable id is queried once across RMS/sequence updates', () => {
    const attempted = new Map<string, VoiceCreditAttempt>();
    const queried: string[] = [];
    const first = collectVoiceBilledTurns(snapshot({ seq: 1 }), 'conv-b');
    const second = collectVoiceBilledTurns(snapshot({ seq: 99 }), 'conv-b');

    expect(
      scheduleVoiceTurnCredits(first, 'conv-b', attempted, (turn) => queried.push(turn.userMessageId))
    ).toBe(1);
    expect(
      scheduleVoiceTurnCredits(second, 'conv-b', attempted, (turn) => queried.push(turn.userMessageId))
    ).toBe(0);
    expect(queried).toEqual(['msg-user-1']);
  });

  test('different conversations use independent dedupe keys', () => {
    const attempted = new Map<string, VoiceCreditAttempt>();
    const queried: string[] = [];
    const turns = [{ turnId: 'turn-1', userMessageId: 'msg-user-1' }];
    expect(scheduleVoiceTurnCredits(turns, 'conv-a', attempted, (turn) => queried.push(turn.userMessageId))).toBe(1);
    expect(scheduleVoiceTurnCredits(turns, 'conv-b', attempted, (turn) => queried.push(turn.userMessageId))).toBe(1);
    expect(queried).toEqual(['msg-user-1', 'msg-user-1']);
  });
});

describe('voice history credit backfill', () => {
  const historyMessage = (overrides: Record<string, unknown> = {}) =>
    ({
      id: 'local-row',
      conversation_id: 'conv-b',
      message_id: 'msg-assistant',
      turn_id: 'turn-history',
      position: 'left',
      type: 'text',
      status: 'finish',
      hidden: false,
      content: { content: 'reply' },
      ...overrides,
    }) as unknown as TMessage;

  test('only durable assistant text rows in this conversation qualify', () => {
    const messages = [
      historyMessage(),
      historyMessage({ id: 'temporary', message_id: undefined }),
      historyMessage({ conversation_id: 'conv-other', message_id: 'msg-other' }),
      historyMessage({ position: 'right', message_id: 'msg-user' }),
      historyMessage({ content: { content: 'task', agentMessage: true } }),
      historyMessage({ status: 'streaming', message_id: 'msg-streaming' }),
      historyMessage({ status: 'pending', message_id: 'msg-pending' }),
      historyMessage({ turn_id: 'turn-history' }),
    ];
    expect(collectVoiceHistoryBilledTurns(messages, 'conv-b')).toEqual([
      { turnId: 'turn-history', userMessageId: 'turn-history' },
    ]);
  });

  test('backfill is bounded and newest-first', () => {
    const messages = Array.from({ length: 16 }, (_, index) =>
      historyMessage({
        id: 'row-' + index,
        message_id: 'assistant-' + index,
        turn_id: 'turn-' + index,
      })
    );
    const turns = collectVoiceHistoryBilledTurns(messages, 'conv-b');
    expect(turns).toHaveLength(12);
    expect(turns[0]).toEqual({ turnId: 'turn-15', userMessageId: 'turn-15' });
    expect(turns[11]).toEqual({ turnId: 'turn-4', userMessageId: 'turn-4' });
  });

  test('live and history duplicates share one Conversation + turn key', () => {
    const attempted = new Map<string, VoiceCreditAttempt>();
    const queried: string[] = [];
    const live = collectVoiceBilledTurns(snapshot(), 'conv-b');
    const history = collectVoiceHistoryBilledTurns(
      [historyMessage({ turn_id: 'msg-user-1' })],
      'conv-b'
    );

    expect(
      scheduleVoiceTurnCredits([...live, ...history], 'conv-b', attempted, (turn) =>
        queried.push(turn.userMessageId)
      )
    ).toBe(1);
    expect(
      scheduleVoiceTurnCredits(history, 'conv-b', attempted, (turn) => queried.push(turn.userMessageId))
    ).toBe(0);
    expect(queried).toEqual(['msg-user-1']);
  });
});
