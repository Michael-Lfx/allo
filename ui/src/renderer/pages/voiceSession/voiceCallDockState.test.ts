/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';
import { resolveVoiceDockPresentation } from './voiceCallDockState';

const boundSnapshot = (overrides: Partial<VoiceSnapshot> = {}): VoiceSnapshot =>
  ({
    instance_id: 'host',
    seq: 1,
    session_id: 'call',
    phase: 'listening',
    connection: 'connected',
    enabled: true,
    acoustic_ready: true,
    dialogue_ready: true,
    playback_ready: true,
    binding: {
      session_id: 'call',
      binding_id: 'binding',
      conversation_id: 'conv-a',
      workspace: null,
    },
    audio: {
      rms_milli: 320,
      peak_milli: 700,
      frames_received: 12,
      last_frame_at_ms: 1000,
      capture_state: 'receiving',
      speech_detected: true,
      asr_ready: true,
      kws_ready: true,
      vad_ready: true,
      barge_in_ready: false,
      barge_in_reason: null,
    },
    projected_turn_ids: [],
    conversation: [],
    error: null,
    ended_reason: null,
    user_text: null,
    assistant_text: null,
    task: null,
    queue: [],
    confirmation: null,
    ...overrides,
  }) as VoiceSnapshot;

const view = (snapshot: VoiceSnapshot | null, overrides: Record<string, unknown> = {}) =>
  resolveVoiceDockPresentation({
    conversationId: 'conv-a',
    snapshot,
    ...overrides,
  });

describe('voice dock presentation mapping', () => {
  test('independent review: an unbound closed capture cannot advertise a working wake word', () => {
    const base = boundSnapshot();
    for (const capture_state of ['closed', 'no_pcm']) {
      const state = view(boundSnapshot({
        phase: 'idle',
        session_id: null,
        binding: null,
        audio: { ...base.audio!, capture_state },
      }));
      expect(state.activity).toBe('none');
      expect(state.labelKey).not.toBe('voice.dock.waitingWake');
    }
  });
  test('independent review: a stalled PCM stream cannot advertise listening or wake readiness', () => {
    const base = boundSnapshot();
    for (const phase of ['listening', 'follow_up', 'idle'] as const) {
      const state = view(boundSnapshot({
        phase,
        audio: { ...base.audio!, capture_state: 'no_pcm', speech_detected: false, rms_milli: 0 },
      }));
      expect(state.activity).toBe('none');
      expect(state.labelKey).not.toBe('voice.dock.listening');
      expect(state.labelKey).not.toBe('voice.dock.followUp');
      expect(state.labelKey).not.toBe('voice.dock.waitingWake');
    }
  });
  test('missing or failed snapshots never claim listening', () => {
    expect(view(null).labelKey).toBe('voice.dock.syncing');
    const failed = view(null, { failed: true });
    expect(failed.tone).toBe('danger');
    expect(failed.alert).toBe(true);
    expect(failed.labelKey).toBe('voice.dock.unavailable');
  });

  test('connection and offline states are distinct from a ready microphone', () => {
    expect(view(boundSnapshot(), { isConnecting: true })).toMatchObject({
      labelKey: 'voice.dock.connecting',
      activity: 'dots',
    });
    expect(view(boundSnapshot({ connection: 'offline' }))).toMatchObject({
      labelKey: 'voice.dock.unavailable',
      tone: 'danger',
    });
    expect(view(boundSnapshot({ connection: 'stopped', enabled: false }))).toMatchObject({
      labelKey: 'voice.dock.notInCall',
      tone: 'neutral',
    });
  });

  test('waiting for wake word requires confirmed KWS readiness', () => {
    const idle = boundSnapshot({ phase: 'idle' });
    expect(view(idle, { conversationId: 'conv-b' }).labelKey).toBe('voice.dock.notInCall');
    expect(view(idle).labelKey).toBe('voice.dock.waitingWake');
    expect(view(idle).activity).toBe('breathe');

    const noKws = boundSnapshot({
      phase: 'idle',
      audio: { ...boundSnapshot().audio!, kws_ready: false },
    });
    expect(view(noKws).labelKey).toBe('voice.dock.notInCall');
  });

  test('listening, follow-up and text-only fallback follow acoustic readiness', () => {
    expect(view(boundSnapshot())).toMatchObject({
      labelKey: 'voice.dock.listening',
      tone: 'active',
      activity: 'wave',
    });
    expect(view(boundSnapshot({ phase: 'follow_up' }))).toMatchObject({
      labelKey: 'voice.dock.followUp',
      tone: 'active',
      activity: 'wave',
    });
    expect(view(boundSnapshot({ acoustic_ready: false }))).toMatchObject({
      labelKey: 'voice.dock.textFollowUp',
      activity: 'none',
    });
  });

  test('processing phases use dots and never show a listening wave', () => {
    for (const [phase, labelKey] of [
      ['transcribing', 'voice.dock.transcribing'],
      ['deciding', 'voice.dock.deciding'],
      ['synthesizing', 'voice.dock.synthesizing'],
      ['dispatching', 'voice.dock.dispatching'],
    ] as const) {
      const state = view(boundSnapshot({ phase }));
      expect(state.labelKey).toBe(labelKey);
      expect(state.activity).toBe('dots');
      expect(state.activity).not.toBe('wave');
    }
  });

  test('interruptibility requires barge-in plus live capture', () => {
    const plain = view(boundSnapshot({ phase: 'speaking' }));
    expect(plain).toMatchObject({
      labelKey: 'voice.dock.speaking',
      canInterrupt: false,
      activity: 'speaker',
    });

    const readyFlagOnly = view(
      boundSnapshot({
        phase: 'speaking',
        audio: { ...boundSnapshot().audio!, barge_in_ready: true, capture_state: 'paused' },
      })
    );
    expect(readyFlagOnly.canInterrupt).toBe(false);

    const interruptible = view(
      boundSnapshot({
        phase: 'speaking',
        audio: { ...boundSnapshot().audio!, barge_in_ready: true },
      })
    );
    expect(interruptible).toMatchObject({
      labelKey: 'voice.dock.speakingInterruptible',
      canInterrupt: true,
      tone: 'active',
    });
  });

  test('approval has priority and stays actionable', () => {
    const approval = view(
      boundSnapshot({
        phase: 'awaiting_confirmation',
        task: {
          command_id: 'cmd-1',
          session_id: 'call',
          acoustic_turn_id: 'turn-1',
          conversation_id: 'conv-a',
          message_id: 'msg-1',
          runtime_turn_id: null,
          status: 'accepted',
          result_ok: null,
          result_text: null,
          error_code: null,
        },
        confirmation: {
          approval_id: 'approval-1',
          msg_id: 'msg-1',
          call_id: 'call-1',
          tool_name: 'read',
          arguments: {},
          expires_at_ms: Date.now() + 60_000,
        },
      })
    );
    expect(approval).toMatchObject({
      labelKey: 'voice.dock.awaitingApproval',
      tone: 'attention',
      needsApproval: true,
      hasDetails: true,
    });
    expect(approval.details.confirmation?.approval_id).toBe('approval-1');
  });

  test('task activity is a badge, not a replacement for the acoustic phase', () => {
    const state = view(
      boundSnapshot({
        task: {
          command_id: 'cmd-1',
          session_id: 'call',
          acoustic_turn_id: 'turn-1',
          conversation_id: 'conv-a',
          message_id: 'msg-1',
          runtime_turn_id: null,
          status: 'accepted',
          result_ok: null,
          result_text: null,
          error_code: null,
        },
      })
    );
    expect(state.labelKey).toBe('voice.dock.listening');
    expect(state.taskRunning).toBe(true);
    expect(state.hasDetails).toBe(true);
  });

  test('a recoverable ASR failure keeps the active task visible', () => {
    const state = view(boundSnapshot({
      error: { stage: 'asr', code: 'asr_timeout' },
      task: {
        command_id: 'cmd-1', session_id: 'call', acoustic_turn_id: 'turn-1',
        conversation_id: 'conv-a', message_id: 'msg-1', runtime_turn_id: null,
        status: 'accepted', result_ok: null, result_text: null, error_code: null,
      },
    }));
    expect(state.labelKey).toBe('voice.audio.asrTimeout');
    expect(state.taskRunning).toBe(true);
    expect(state.details.task?.command_id).toBe('cmd-1');
  });

  test('capture interruption becomes a verified danger state', () => {
    const state = view(
      boundSnapshot({
        audio: { ...boundSnapshot().audio!, capture_state: 'closed' },
      })
    );
    expect(state.tone).toBe('danger');
    expect(state.alert).toBe(true);
    expect(state.activity).toBe('none');
    expect(state.labelKey).toContain('voice.audio.captureClosed');
  });

  test('an ended call can still expose background queue and task facts', () => {
    const state = view(
      boundSnapshot({
        phase: 'idle',
        ended_reason: 'user_stop',
        task: {
          command_id: 'cmd-1',
          session_id: 'call',
          acoustic_turn_id: 'turn-1',
          conversation_id: 'conv-a',
          message_id: 'msg-1',
          runtime_turn_id: null,
          status: 'unknown',
          result_ok: null,
          result_text: null,
          error_code: 'task_receipt_missing',
        },
        queue: [
          {
            task: {
              command_id: 'cmd-2',
              session_id: 'call',
              acoustic_turn_id: 'turn-2',
              conversation_id: 'conv-a',
              message_id: 'msg-2',
              runtime_turn_id: null,
              status: 'queued',
              result_ok: null,
              result_text: null,
              error_code: null,
            },
            display_text: 'queued follow-up',
            workspace: 'D:/workspace',
            position: 1,
          },
        ],
      })
    );
    expect(state.labelKey).toBe('voice.dock.notInCall');
    expect(state.hasDetails).toBe(true);
    expect(state.details.queue).toHaveLength(1);
    expect(state.details.task?.command_id).toBe('cmd-1');
  });

  test('browsing conversation B cannot read call A task or approval content', () => {
    const state = resolveVoiceDockPresentation({
      conversationId: 'conv-b',
      snapshot: boundSnapshot({
        task: {
          command_id: 'cmd-a',
          session_id: 'call',
          acoustic_turn_id: 'turn-a',
          conversation_id: 'conv-a',
          message_id: 'msg-a',
          runtime_turn_id: null,
          status: 'accepted',
          result_ok: null,
          result_text: 'A private result',
          error_code: null,
        },
        confirmation: {
          approval_id: 'approval-a',
          msg_id: 'msg-a',
          call_id: 'call-a',
          tool_name: 'read',
          arguments: { path: 'D:/private' },
          expires_at_ms: Date.now() + 60_000,
        },
        queue: [
          {
            task: {
              command_id: 'cmd-a2',
              session_id: 'call',
              acoustic_turn_id: 'turn-a2',
              conversation_id: 'conv-a',
              message_id: 'msg-a2',
              runtime_turn_id: null,
              status: 'queued',
              result_ok: null,
              result_text: null,
              error_code: null,
            },
            display_text: 'A private queued text',
            workspace: 'D:/private',
            position: 1,
          },
        ],
      }),
      isActiveElsewhere: true,
      hasReturnAction: true,
    });
    expect(state.labelKey).toBe('voice.dock.activeElsewhere');
    expect(state.showReturn).toBe(true);
    expect(state.hasDetails).toBe(false);
    expect(state.details.task).toBeNull();
    expect(state.details.queue).toEqual([]);
    expect(state.details.confirmation).toBeNull();
  });

  test('history failure remains visible and recoverable without hijacking the phase', () => {
    const state = view(boundSnapshot(), { historyFailed: true });
    expect(state.labelKey).toBe('voice.dock.listening');
    expect(state.syncFailed).toBe(true);
    expect(state.hasDetails).toBe(true);
  });
});
