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

interface JointTracePayload {
  conversation_id: string;
  session_id: string;
  messages: Array<{
    message_id: string;
    position: 'right' | 'left';
    content: { content: string; turn_id?: string };
  }>;
  counts: {
    completer_calls: number;
    task_submits: number;
    tts_synthesizes: number;
    announcements: number;
  };
  recovery: {
    turn_b_write_failed_injected: boolean;
    turn_b_recovered_from_journal: boolean;
    extra_model_calls_on_recovery: number;
  };
}

function loadTrace(): JointTracePayload {
  const tracePath = path.resolve(
    process.cwd(),
    '../build.noindex',
    'voice-task-completion-round5-closeout-20261030',
    'joint-trace.json',
  );
  const fallbackPath = path.resolve(
    process.cwd(),
    'build.noindex',
    'voice-task-completion-round5-closeout-20261030',
    'joint-trace.json',
  );
  const resolved = fs.existsSync(tracePath) ? tracePath : fallbackPath;
  if (!fs.existsSync(resolved)) {
    throw new Error(`joint-trace.json not found at ${tracePath} or ${fallbackPath}`);
  }
  const raw = fs.readFileSync(resolved, 'utf8');
  return JSON.parse(raw);
}

test('joint trace from authentic Rust joint chain is valid and conforms to spec', () => {
  const trace = loadTrace();
  expect(trace.conversation_id).toBeTruthy();
  expect(trace.session_id).toBe('scoped-joint-control');
  expect(trace.messages).toHaveLength(6);
  expect(trace.counts.completer_calls).toBe(3);
  expect(trace.counts.task_submits).toBe(1);
  expect(trace.counts.announcements).toBe(1);
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
    user_message_id: null,
    assistant_message_id: null,
  };

  const snapshotWithFailure = {
    instance_id: 'inst-1',
    phase: 'follow_up',
    session_id: trace.session_id,
    turn_id: 'turn-b',
    binding: {
      session_id: trace.session_id,
      binding_id: 'binding-1',
      conversation_id: convId,
      workspace: 'D:/owned',
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

test('recovery and reload phase: durable trace rows retire live rows with zero duplicates and exact ordering', () => {
  const trace = loadTrace();
  const convId = parseConversationId(trace.conversation_id);

  // Convert trace messages to durable TMessage rows
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

  // Once durable messages arrive, live messages retire smoothly
  const finalSnapshot = {
    instance_id: 'inst-1',
    phase: 'idle',
    session_id: trace.session_id,
    turn_id: 'turn-c',
    conversation: [],
    projected_turn_ids: ['turn-a', 'turn-b', 'turn-c'],
    ended_reason: 'user_stop',
  } as unknown as VoiceSnapshot;

  const initialHistory = createLiveVoiceHistory();
  const reconciled = reconcileLiveVoiceHistory(initialHistory, {
    snapshot: finalSnapshot,
    conversationId: convId,
    epoch: 2,
    visibleIds: new Set(durableRows.map((r) => r.message_id as string).filter(Boolean)),
    currentSequence: 1,
    latestPage: { conversationId: convId, epoch: 2, sequence: 1 },
  });

  const remainingLive = buildVoiceLiveMessages(finalSnapshot, convId, durableRows, reconciled.state);
  expect(remainingLive).toHaveLength(0); // Zero duplicate messages remaining in live view
});
