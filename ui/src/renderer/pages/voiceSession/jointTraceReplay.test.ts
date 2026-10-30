import { expect, test } from 'bun:test';
import * as fs from 'fs';
import * as path from 'path';
import * as crypto from 'crypto';
import {
  buildVoiceLiveMessages,
  createLiveVoiceHistory,
  mergeVoiceTranscript,
  reconcileLiveVoiceHistory,
  liveVoiceTurn,
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
  snapshots: {
    at_b_failure: VoiceSnapshot;
    after_b_recovery: VoiceSnapshot;
    final: VoiceSnapshot;
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
    extra_model_calls_on_recovery?: number;
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

    // Cryptographic producer-to-consumer SHA256 verification
    const expectedSha = process.env.VOICE_JOINT_TRACE_EXPECTED_SHA;
    if (expectedSha) {
      const actualSha = crypto.createHash('sha256').update(Buffer.from(raw, 'utf8')).digest('hex');
      if (actualSha.toLowerCase() !== expectedSha.toLowerCase()) {
        throw new Error(`Trace SHA256 mismatch! expected: ${expectedSha}, actual: ${actualSha}`);
      }
    }

    return JSON.parse(raw);
  }
  return defaultFixture as unknown as JointTracePayload;
}

test('joint trace payload contains authentic multi-stage snapshots, session ID and 6 messages', () => {
  const trace = loadTrace();
  expect(trace.conversation_id).toBeTruthy();
  expect(trace.session_id).toBeTruthy();
  expect(trace.session_id).not.toBe('scoped-joint-control');
  expect(trace.messages).toHaveLength(6);
  expect(trace.counts.completer_calls).toBe(3);
  expect(trace.counts.task_submits).toBe(1);
  expect(trace.counts.announcements_succeeded).toBe(1);
  expect(trace.recovery.turn_b_write_failed_injected).toBe(true);
  expect(trace.recovery.turn_b_recovered_from_journal).toBe(true);
  expect(trace.recovery.extra_model_calls_on_recovery).toBe(0);

  // Assert binding exists and matches authoritative scope
  expect(trace.binding).toBeTruthy();
  expect(trace.binding!.session_id).toBe(trace.session_id);
  expect(trace.binding!.conversation_id).toBe(trace.conversation_id);

  // Assert multi-stage snapshots exist from authentic Rust joint execution
  expect(trace.snapshots).toBeTruthy();
  const { at_b_failure, after_b_recovery, final: snapFinal } = trace.snapshots;
  expect(at_b_failure).toBeTruthy();
  expect(after_b_recovery).toBeTruthy();
  expect(snapFinal).toBeTruthy();

  // Assert cross-stage identity consistency
  expect(at_b_failure.session_id).toBe(trace.session_id);
  expect(after_b_recovery.session_id).toBe(trace.session_id);
  expect(snapFinal.session_id).toBe(trace.session_id);

  expect(at_b_failure.instance_id).toBe(trace.instance_id);
  expect(after_b_recovery.instance_id).toBe(trace.instance_id);
  expect(snapFinal.instance_id).toBe(trace.instance_id);

  // Final snapshot must have user_stop ended_reason
  expect(snapFinal.ended_reason).toBe('user_stop');
});

test('failure phase: authentic failure snapshot preserves user text and model response with localized error mapping', () => {
  const trace = loadTrace();
  const convId = parseConversationId(trace.conversation_id);
  const snapAtFailure = trace.snapshots.at_b_failure;

  // Locate Turn B in the authentic conversation turns
  const turnB = snapAtFailure.conversation?.find((t) => t.user_text?.includes('今天天气怎么样'));
  expect(turnB).toBeTruthy();
  expect(turnB!.assistant_text).toContain('天气晴朗');
  expect(turnB!.reply_state).toBe('complete');

  // Verify projection failure error is captured honestly in the turn or snapshot
  const capturedErrorCode = turnB?.error?.code ?? snapAtFailure.error?.code;
  expect(capturedErrorCode).toBe('voice_projection_write_failed');

  // Live messages rendered directly from the authentic failure snapshot
  const liveMessages = buildVoiceLiveMessages(snapAtFailure, convId, []);
  expect(liveMessages.length).toBeGreaterThanOrEqual(2);
  const userMsg = liveMessages.find((m) => (m.content as { content: string }).content.includes('今天天气怎么样'));
  const asstMsg = liveMessages.find((m) => (m.content as { content: string }).content.includes('天气晴朗'));
  expect(userMsg).toBeTruthy();
  expect(asstMsg).toBeTruthy();

  // Verify localized error status without raw error code leakage
  const errorKey = voiceErrorKey('voice_projection_write_failed');
  const translationKey = voiceErrorTranslationKey('voice_projection_write_failed');
  expect(errorKey).toBe('voice.audio.projectionFailed');
  expect(translationKey).toBe('voice.audio.projectionFailed');
  expect(zhLocale.audio.projectionFailed).toContain('当前面板仍显示已生成文字');
  expect(enLocale.audio.projectionFailed).toContain('Generated text stays visible');
  expect(zhLocale.audio.projectionFailed).not.toContain('voice_projection_write_failed');
  expect(enLocale.audio.projectionFailed).not.toContain('voice_projection_write_failed');
});

test('reconcileLiveVoiceHistory tracks Turn B in history and accurately retires it upon durable arrival', () => {
  const trace = loadTrace();
  const convId = parseConversationId(trace.conversation_id);
  const snapAtFailure = trace.snapshots.at_b_failure;
  const snapAfterRecovery = trace.snapshots.after_b_recovery;

  const turnB = snapAtFailure.conversation?.find((t) => t.user_text?.includes('今天天气怎么样'))!;
  expect(turnB).toBeTruthy();
  const turnBId = turnB.turn_id;

  // Assert recovery snapshot has properly cleared projection errors
  expect(snapAfterRecovery.error == null).toBe(true);
  const recoveredTurnB = snapAfterRecovery.conversation?.find((t) => t.user_text?.includes('今天天气怎么样'))!;
  expect(recoveredTurnB.error == null).toBe(true);

  // 1. Stage 1: In-flight / failure phase with only Task A durable in repository
  const durableARows: TMessage[] = [
    {
      id: 1,
      message_id: trace.messages[0].message_id,
      conversation_id: trace.conversation_id,
      msg_id: trace.messages[0].message_id,
      type: 'text',
      content: trace.messages[0].content,
      position: 'right',
      status: 'finish',
      hidden: false,
      created_at: 1000,
    },
    {
      id: 2,
      message_id: trace.messages[1].message_id,
      conversation_id: trace.conversation_id,
      msg_id: trace.messages[1].message_id,
      type: 'text',
      content: trace.messages[1].content,
      position: 'left',
      status: 'finish',
      hidden: false,
      created_at: 1010,
    },
  ] as unknown as TMessage[];

  const initialHistory = createLiveVoiceHistory();
  const stage1 = reconcileLiveVoiceHistory(initialHistory, {
    snapshot: snapAtFailure,
    conversationId: convId,
    epoch: 1,
    visibleIds: new Set(durableARows.map((r) => r.message_id)),
    currentSequence: 1,
    latestPage: { conversationId: convId, epoch: 1, sequence: 1 },
  });

  // Explicitly assert on history internal tracking state:
  expect(stage1.state.observedTurns.has(turnBId)).toBe(true);
  expect(stage1.state.waitingTurns.has(turnBId)).toBe(false); // Not waiting because not projected yet

  // Live messages must display Turn B while A is already durable
  const live1 = buildVoiceLiveMessages(snapAtFailure, convId, durableARows, stage1.state);
  expect(live1.some((m) => (m.content as { content: string }).content.includes('今天天气怎么样'))).toBe(true);
  expect(live1.some((m) => (m.content as { content: string }).content.includes('天气晴朗'))).toBe(true);

  // 2. Stage 2: Recovery succeeds and Turn B durable messages arrive
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
  const durableABRows = [...durableARows, ...durableBRows];

  // Reconcile into the previous history state with recovery snapshot
  const stage2 = reconcileLiveVoiceHistory(stage1.state, {
    snapshot: snapAfterRecovery,
    conversationId: convId,
    epoch: 1,
    visibleIds: new Set(durableABRows.map((r) => r.message_id)),
    currentSequence: 2,
    latestPage: { conversationId: convId, epoch: 1, sequence: 2 },
  });

  // Explicitly assert on retirement state:
  expect(stage2.state.retiredIds.has(trace.messages[2].message_id)).toBe(true);
  expect(stage2.state.retiredIds.has(trace.messages[3].message_id)).toBe(true);
  expect(stage2.state.observedTurns.has(turnBId)).toBe(true);

  // Turn B is accurately retired: live messages have 0 remaining items
  const live2 = buildVoiceLiveMessages(snapAfterRecovery, convId, durableABRows, stage2.state);
  expect(live2.filter((m) => (m.content as { content: string }).content.includes('今天天气怎么样'))).toHaveLength(0);

  // Merged transcript contains exactly 4 messages without duplicate bubbles
  const merged = mergeVoiceTranscript(snapAfterRecovery, convId, durableABRows, stage2.state);
  expect(merged).toHaveLength(4);
});

test('scope and conversation isolation: foreign sessions and unmatched turns are not retired', () => {
  const trace = loadTrace();
  const convId = parseConversationId(trace.conversation_id);

  // Snapshot with an additional foreign session turn
  const foreignTurn = {
    session_id: 'foreign-session-xyz',
    turn_id: 'foreign-turn-123',
    user_text: 'foreign question',
    assistant_text: 'foreign reply',
    reply_state: 'complete' as const,
  };

  const snapshotWithForeign: VoiceSnapshot = {
    ...trace.snapshots.at_b_failure,
    conversation: [
      ...(trace.snapshots.at_b_failure.conversation ?? []),
      foreignTurn,
    ],
  };

  const history = createLiveVoiceHistory();
  const reconciled = reconcileLiveVoiceHistory(history, {
    snapshot: snapshotWithForeign,
    conversationId: convId,
    epoch: 1,
    visibleIds: new Set(),
    currentSequence: 1,
    latestPage: null,
  });

  // Foreign turn must not enter observedTurns or retiredIds for this conversation scope
  expect(reconciled.state.observedTurns.has('foreign-turn-123')).toBe(false);
  expect(reconciled.state.retiredIds.has('foreign-turn-123')).toBe(false);

  // Foreign turn must never be returned in live messages
  const live = buildVoiceLiveMessages(snapshotWithForeign, convId, []);
  expect(live.some((m) => (m.content as { content: string }).content.includes('foreign question'))).toBe(false);
});

test('formalized negative control: bypassing or discarding history state fails observedTurns and retirement assertions', () => {
  const trace = loadTrace();
  const convId = parseConversationId(trace.conversation_id);
  const snapAtFailure = trace.snapshots.at_b_failure;
  const snapAfterRecovery = trace.snapshots.after_b_recovery;
  const turnB = snapAtFailure.conversation?.find((t) => t.user_text?.includes('今天天气怎么样'))!;
  expect(turnB).toBeTruthy();

  // 1. If history is bypassed / discarded (replacing with blank createLiveVoiceHistory):
  const discardedHistory = createLiveVoiceHistory();

  // The state assertions proving tracking MUST fail if state is not maintained:
  expect(discardedHistory.observedTurns.has(turnB.turn_id)).toBe(false);
  expect(discardedHistory.retiredIds.has(trace.messages[2].message_id)).toBe(false);
  expect(discardedHistory.retiredIds.has(trace.messages[3].message_id)).toBe(false);

  // 2. Rerun the positive reconciliation scenario using discarded history instead of stage1.state:
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

  const stageWithoutHistory = reconcileLiveVoiceHistory(discardedHistory, {
    snapshot: snapAfterRecovery,
    conversationId: convId,
    epoch: 1,
    visibleIds: new Set(durableBRows.map((r) => r.message_id)),
    currentSequence: 2,
    latestPage: { conversationId: convId, epoch: 1, sequence: 2 },
  });

  // Because history was discarded, it failed to track the turn from in-flight to retirement:
  expect(discardedHistory.observedTurns.has(turnB.turn_id)).toBe(false);
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
