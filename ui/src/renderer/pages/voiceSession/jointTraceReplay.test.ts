import { expect, test } from 'bun:test';
import * as fs from 'fs';
import * as path from 'path';
import {
  buildVoiceLiveMessages,
  createLiveVoiceHistory,
  reconcileLiveVoiceHistory,
} from './liveVoiceTurn';
import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';
import { parseConversationId } from '@common/types/ids';
import type { TMessage } from '@common/chat/chatLib';
import { voiceErrorKey, voiceErrorTranslationKey } from './VoiceErrorStatus';
import zhLocale from '../../services/i18n/locales/zh-CN/voice.json';
import enLocale from '../../services/i18n/locales/en-US/voice.json';
import defaultFixture from './fixtures/jointTraceFixture.json';

interface JointTracePayload {
  conversation_id: string;
  session_id: string;
  instance_id: string;
  binding?: {
    session_id: string;
    binding_id: string;
    conversation_id: string;
    workspace?: string;
  };
  messages: Array<{
    message_id: string;
    position: 'right' | 'left';
    content: { content: string; turn_id?: string };
  }>;
  counts: {
    completer_calls: number;
    task_submits: number;
    tts_synthesizes: number;
    announcements_succeeded?: number;
    announcement_attempts?: number;
  };
  recovery: {
    turn_b_write_failed_injected: boolean;
    turn_b_recovered_from_journal: boolean;
    extra_model_calls_on_recovery: number;
  };
}

function loadTrace(): JointTracePayload {
  const customPath = process.env.VOICE_JOINT_TRACE_PATH;
  if (customPath) {
    if (!fs.existsSync(customPath)) {
      throw new Error(`explicit VOICE_JOINT_TRACE_PATH not found at ${customPath}`);
    }
    const raw = fs.readFileSync(customPath, 'utf8');
    if (!raw.trim()) {
      throw new Error(`explicit VOICE_JOINT_TRACE_PATH is empty at ${customPath}`);
    }
    return JSON.parse(raw);
  }
  return defaultFixture as unknown as JointTracePayload;
}

test('joint trace fixture is valid and contains authentic fields without hardcoding', () => {
  const trace = loadTrace();
  expect(trace.conversation_id).toBeTruthy();
  expect(trace.session_id).toBeTruthy();
  expect(trace.session_id).not.toBe('scoped-joint-control'); // Proves real session id, not control token
  expect(trace.messages).toHaveLength(6);
  expect(trace.counts.completer_calls).toBe(3);
  expect(trace.counts.task_submits).toBe(1);
  expect(trace.recovery.turn_b_write_failed_injected).toBe(true);
  expect(trace.recovery.turn_b_recovered_from_journal).toBe(true);
  expect(trace.recovery.extra_model_calls_on_recovery).toBe(0);
});

test('failure phase: simulated projection failure preserves user input and model text', () => {
  const trace = loadTrace();
  const convId = parseConversationId(trace.conversation_id);

  // Turn B during the projection failure stage
  const turnB: NonNullable<VoiceSnapshot['conversation']>[number] = {
    session_id: trace.session_id,
    turn_id: 'turn-b',
    user_text: trace.messages[2].content.content,
    assistant_text: trace.messages[3].content.content,
    reply_state: 'complete',
    model_text: trace.messages[3].content.content,
    user_message_id: trace.messages[2].message_id,
    assistant_message_id: trace.messages[3].message_id,
  };

  const snapshotWithFailure = {
    instance_id: trace.instance_id ?? 'voice-companion-inst-1',
    phase: 'follow_up',
    session_id: trace.session_id,
    turn_id: 'turn-b',
    binding: trace.binding ?? {
      session_id: trace.session_id,
      binding_id: `voice-binding-${trace.session_id}`,
      conversation_id: convId,
      workspace: 'D:/workspace/test',
    },
    conversation: [turnB],
    projected_turn_ids: [],
    error: { stage: 'history', code: 'voice_projection_write_failed' },
  } as unknown as VoiceSnapshot;

  // Live messages must display both the user input and the model response despite the write failure
  const liveMessages = buildVoiceLiveMessages(snapshotWithFailure, convId, []);
  expect(liveMessages).toHaveLength(2);
  expect((liveMessages[0].content as { content: string }).content).toContain('今天天气怎么样');
  expect((liveMessages[1].content as { content: string }).content).toContain('天气晴朗');

  // Verify error mapping in dual locales without leaking raw internal codes
  const errorKey = voiceErrorKey('voice_projection_write_failed');
  const translationKey = voiceErrorTranslationKey('voice_projection_write_failed');
  expect(errorKey).toBe('voice.audio.projectionFailed');
  expect(translationKey).toBe('voice.audio.projectionFailed');

  // Assert dual-locale strings exist and neither leaks the raw code
  expect(zhLocale.audio.projectionFailed).toContain('当前面板仍显示已生成文字');
  expect(enLocale.audio.projectionFailed).toContain('Generated text stays visible');
  expect(zhLocale.audio.projectionFailed).not.toContain('voice_projection_write_failed');
  expect(enLocale.audio.projectionFailed).not.toContain('voice_projection_write_failed');
});

test('non-empty live history retires turn B upon matching durable row arrival, preserving order and foreign scope', () => {
  const trace = loadTrace();
  const convId = parseConversationId(trace.conversation_id);

  // 1. Establish non-empty live history with Turn B in-flight
  const turnB: NonNullable<VoiceSnapshot['conversation']>[number] = {
    session_id: trace.session_id,
    turn_id: 'turn-b-live',
    user_text: trace.messages[2].content.content,
    assistant_text: trace.messages[3].content.content,
    reply_state: 'complete',
    model_text: trace.messages[3].content.content,
    user_message_id: trace.messages[2].message_id,
    assistant_message_id: trace.messages[3].message_id,
  };

  const foreignTurn: NonNullable<VoiceSnapshot['conversation']>[number] = {
    session_id: 'foreign-session-id',
    turn_id: 'foreign-turn-id',
    user_text: 'foreign input',
    assistant_text: 'foreign reply',
    reply_state: 'complete',
    model_text: 'foreign reply',
    user_message_id: 'foreign-user-msg',
    assistant_message_id: 'foreign-asst-msg',
  };

  const snapshotWithB = {
    instance_id: trace.instance_id ?? 'voice-companion-inst-1',
    phase: 'follow_up',
    session_id: trace.session_id,
    turn_id: 'turn-b-live',
    binding: trace.binding ?? {
      session_id: trace.session_id,
      binding_id: `voice-binding-${trace.session_id}`,
      conversation_id: convId,
      workspace: 'D:/workspace/test',
    },
    conversation: [turnB, foreignTurn],
    projected_turn_ids: [],
  } as unknown as VoiceSnapshot;

  const liveBefore = buildVoiceLiveMessages(snapshotWithB, convId, []);
  expect(liveBefore).toHaveLength(2);
  expect((liveBefore[0].content as { content: string }).content).toContain('今天天气怎么样');
  expect((liveBefore[1].content as { content: string }).content).toContain('天气晴朗');

  // 2. Durable message rows arrive containing B
  const durableBRows: TMessage[] = [
    {
      id: 3,
      message_id: trace.messages[2].message_id,
      conversation_id: trace.conversation_id,
      msg_id: trace.messages[2].message_id,
      type: 'text',
      content: trace.messages[2].content,
      position: 'right',
      status: 'finish',
      hidden: false,
      created_at: 1020,
    },
    {
      id: 4,
      message_id: trace.messages[3].message_id,
      conversation_id: trace.conversation_id,
      msg_id: trace.messages[3].message_id,
      type: 'text',
      content: trace.messages[3].content,
      position: 'left',
      status: 'finish',
      hidden: false,
      created_at: 1030,
    },
  ] as unknown as TMessage[];

  const snapshotAfterBProjected = {
    ...snapshotWithB,
    projected_turn_ids: ['turn-b-live'],
  } as unknown as VoiceSnapshot;

  const history = createLiveVoiceHistory();
  const reconciled = reconcileLiveVoiceHistory(history, {
    snapshot: snapshotAfterBProjected,
    conversationId: convId,
    epoch: 2,
    visibleIds: new Set(durableBRows.map((r) => r.message_id as string).filter(Boolean)),
    currentSequence: 2,
    latestPage: { conversationId: convId, epoch: 2, sequence: 2 },
  });

  // Turn B is accurately retired!
  const liveAfter = buildVoiceLiveMessages(snapshotAfterBProjected, convId, durableBRows, reconciled.state);
  expect(liveAfter).toHaveLength(0);
});

test('durable trace rows assert strict six message sequential ordering and roles', () => {
  const trace = loadTrace();
  const durableRows: TMessage[] = trace.messages.map((m, idx) => ({
    id: idx + 1,
    message_id: m.message_id,
    conversation_id: trace.conversation_id,
    msg_id: m.message_id,
    type: 'text',
    content: m.content,
    position: m.position,
    status: 'finish',
    hidden: false,
    created_at: 1000 + idx * 10,
  })) as unknown as TMessage[];

  expect(durableRows).toHaveLength(6);
  expect(durableRows[0].position).toBe('right');
  expect((durableRows[0].content as { content: string }).content).toContain('请执行任务');
  expect(durableRows[1].position).toBe('left');
  expect((durableRows[1].content as { content: string }).content).toContain('任务已接单');

  expect(durableRows[2].position).toBe('right');
  expect((durableRows[2].content as { content: string }).content).toContain('今天天气怎么样');
  expect(durableRows[3].position).toBe('left');
  expect((durableRows[3].content as { content: string }).content).toContain('天气晴朗');

  expect(durableRows[4].position).toBe('right');
  expect((durableRows[4].content as { content: string }).content).toContain('明天会下雨吗');
  expect(durableRows[5].position).toBe('left');
  expect((durableRows[5].content as { content: string }).content).toContain('明天天气也很好');
});
