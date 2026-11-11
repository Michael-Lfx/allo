import { describe, expect, test } from 'bun:test';
import type { IMessageText, TMessage } from '@/common/chat/chatLib';
import { parseConversationId } from '@/common/types/ids';
import type { VoiceTaskSnapshot } from '@/common/adapter/voiceCompanion';
import {
  buildVoiceKeyboardWaitUserPreview,
  hasAuthoritativeVoiceReply,
  isVoiceKeyboardWaitExpired,
  phaseFromDeliveryError,
  VOICE_KEYBOARD_WAIT_MAX_MS,
  phaseFromVoiceTask,
  type VoiceKeyboardWait,
} from './voiceKeyboardWait';

const conversationId = parseConversationId('019b0000-0000-7000-8000-000000000001');
const task = (overrides: Partial<VoiceTaskSnapshot> = {}): VoiceTaskSnapshot => ({
  command_id: 'cmd-wait',
  session_id: 'session',
  acoustic_turn_id: 'turn',
  conversation_id: conversationId,
  message_id: 'user-message',
  runtime_turn_id: 'runtime-turn',
  status: 'accepted',
  result_ok: null,
  result_text: null,
  error_code: null,
  ...overrides,
});
const wait = (overrides: Partial<VoiceKeyboardWait> = {}): VoiceKeyboardWait => ({
  conversationId,
  commandId: 'cmd-wait',
  text: '你好',
  phase: 'sending',
  startedAt: 1000,
  task: null,
  ...overrides,
});
const user = (): IMessageText => ({
  id: 'persisted-user',
  message_id: 'user-message' as IMessageText['message_id'],
  msg_id: 'user-message' as IMessageText['msg_id'],
  type: 'text',
  position: 'right',
  conversation_id: conversationId,
  status: 'finish',
  created_at: 1,
  content: { content: '你好' },
});
const reply = (): IMessageText => ({
  ...user(),
  id: 'persisted-reply',
  message_id: 'reply-message' as IMessageText['message_id'],
  msg_id: 'reply-message' as IMessageText['msg_id'],
  position: 'left',
  content: { content: '任务结果' },
});

describe('voice keyboard wait view state', () => {
  test('first render keeps only the right user preview in the transcript', () => {
    const rows = buildVoiceKeyboardWaitUserPreview(wait(), []);
    expect(rows.map(row => row.position)).toEqual(['right']);
    expect(rows[0].type).toBe('text');
    expect((rows[0] as IMessageText).content.content).toBe('你好');
  });

  test('uses the authoritative user row; the left row is rendered outside the transcript', () => {
    expect(buildVoiceKeyboardWaitUserPreview(wait({ task: task() }), [user()])).toEqual([]);
    expect(hasAuthoritativeVoiceReply([user()], task())).toBe(false);
    expect(hasAuthoritativeVoiceReply([user(), reply()], task())).toBe(true);
    expect(buildVoiceKeyboardWaitUserPreview(wait({ task: task() }), [user(), reply()])).toEqual([]);
  });

  test('maps real task phases without inventing success', () => {
    expect(phaseFromVoiceTask(task({ status: 'queued' }))).toBe('queued');
    expect(phaseFromVoiceTask(task({ status: 'delivering' }))).toBe('delivering');
    expect(phaseFromVoiceTask(task({ status: 'cancelling' }))).toBe('cancelling');
    expect(phaseFromVoiceTask(task({ status: 'completed' }))).toBe('syncing');
    expect(phaseFromVoiceTask(task({ status: 'failed' }))).toBe('failed');
    expect(phaseFromVoiceTask(null)).toBe('unknown');
  });

  test('unknown delivery codes and network failures continue polling; hard rejects fail', () => {
    expect(phaseFromDeliveryError(new Error('task_delivery_pending'))).toBe('unknown');
    expect(phaseFromDeliveryError(new Error('task_delivery_unknown'))).toBe('unknown');
    expect(phaseFromDeliveryError(new Error('Failed to fetch'))).toBe('unknown');
    expect(phaseFromDeliveryError(new Error('binding_invalid'))).toBe('failed');
    expect(phaseFromDeliveryError(new Error('task_command_conflict'))).toBe('failed');
  });

  test('a persisted right row with the same text suppresses the local preview', () => {
    const persistedRight = {
      ...user(),
      message_id: undefined,
      msg_id: undefined,
      created_at: 1_050,
    } as unknown as IMessageText;
    const rows = buildVoiceKeyboardWaitUserPreview(wait(), [persistedRight]);
    expect(rows).toEqual([]);
  });

  test('completed-before-message keeps the left wait, then a durable reply removes it once', () => {
    const completed = task({ status: 'completed' });
    expect(buildVoiceKeyboardWaitUserPreview(wait({ task: completed, phase: 'syncing' }), [user()])).toEqual([]);
    expect(hasAuthoritativeVoiceReply([user()], completed)).toBe(false);
    expect(hasAuthoritativeVoiceReply([user(), reply()], completed)).toBe(true);
    expect(buildVoiceKeyboardWaitUserPreview(wait({ task: completed, phase: 'syncing' }), [user(), reply()])).toHaveLength(0);
  });

  test('the single wait row has one absolute 108s bound', () => {
    expect(isVoiceKeyboardWaitExpired({ startedAt: 1_000 }, 1_000 + 107_999)).toBe(false);
    expect(isVoiceKeyboardWaitExpired({ startedAt: 1_000 }, 1_000 + VOICE_KEYBOARD_WAIT_MAX_MS)).toBe(true);
  });

  test('foreign command identity is ignored by the reply matcher', () => {
    const foreign = user();
    foreign.message_id = 'foreign-message' as TMessage['message_id'];
    foreign.msg_id = 'foreign-message' as TMessage['msg_id'];
    expect(hasAuthoritativeVoiceReply([foreign, reply()], task())).toBe(false);
  });
});
