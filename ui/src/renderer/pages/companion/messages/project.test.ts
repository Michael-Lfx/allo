import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { parseConversationId, parseMessageId } from '@/common/types/ids';
import type { I18nKey } from '@/renderer/services/i18n/i18n-keys';
import { petMessageFromAutoWork, petMessageFromTurnCompleted, petMessageFromTurnStarted, projectOrClear } from './project';
import { foldIncomingPetMessage, leadingPetPhase, mergePetMessage, motionFromMood, resolveCompanionMotion } from './motion';
import { truncateDetail, type PetMessage } from './types';

const key = (value: string): I18nKey => value as I18nKey;

const CID = parseConversationId('019b0000-0000-7000-8000-0000000000aa');
const TID = parseMessageId('019b0000-0000-7000-8000-0000000000bb');

const baseRuntime = {
  state: 'running' as const,
  can_send_message: false,
  has_runtime: true,
  is_processing: true,
  pending_confirmations: 0,
};

describe('pet message projection', () => {
  test('ignores companion-owned turns so the chat bubble stays the sole voice', () => {
    expect(
      petMessageFromTurnStarted({
        conversation_id: CID,
        turn_id: TID,
        status: 'running',
        phase: 'thinking',
        state: 'ai_generating',
        detail: 'planning',
        can_send_message: false,
        runtime: baseRuntime,
        companion: true,
      })
    ).toBeNull();
  });

  test('maps admission starting to preparing instead of a generic running label', () => {
    const msg = petMessageFromTurnStarted({
      conversation_id: CID,
      turn_id: TID,
      status: 'running',
      phase: 'starting',
      state: 'initializing',
      detail: '',
      can_send_message: false,
      runtime: baseRuntime,
    });
    expect(msg?.phase).toBe('queued');
    expect(msg?.titleKey).toBe('nomi.petMessage.conversation.preparing');
  });

  test('maps a main-window turn phase to a running pet message', () => {
    const msg = petMessageFromTurnStarted({
      conversation_id: CID,
      turn_id: TID,
      status: 'running',
      phase: 'tooling',
      state: 'ai_generating',
      detail: 'calling web_search with a very long operator-facing explanation that must be clipped',
      can_send_message: false,
      runtime: baseRuntime,
    });
    expect(msg?.source).toBe('conversation');
    expect(msg?.phase).toBe('running');
    expect(msg?.titleKey).toBe('nomi.petMessage.conversation.tooling');
    expect(msg?.href).toBe(`/conversation/${CID}`);
    expect(msg?.detail?.endsWith('…')).toBe(true);
  });

  test('maps waiting_permission to the waiting phase', () => {
    const msg = petMessageFromTurnStarted({
      conversation_id: CID,
      turn_id: TID,
      status: 'running',
      phase: 'waiting_permission',
      state: 'ai_waiting_confirmation',
      detail: '',
      can_send_message: false,
      runtime: { ...baseRuntime, state: 'waiting_confirmation' },
    });
    expect(msg?.phase).toBe('waiting');
    expect(msg?.titleKey).toBe('nomi.petMessage.conversation.waiting');
  });

  test('maps a finished turn to completed', () => {
    const msg = petMessageFromTurnCompleted({
      conversation_id: CID,
      turn_id: TID,
      status: 'finished',
      state: 'ai_waiting_input',
      detail: '',
      can_send_message: true,
      runtime: { ...baseRuntime, state: 'idle', is_processing: false },
      workspace: '',
      model: { platform: '', name: '', use_model: '' },
      last_message: { content: null, created_at: 0 },
    });
    expect(msg?.phase).toBe('completed');
  });

  test('wires the live message.stream projector so process frames can replace admission', () => {
    const source = readFileSync(new URL('./project.ts', import.meta.url), 'utf8');
    expect(source.includes('conversation.responseStream.on')).toBe(true);
    expect(source.includes('petMessageFromResponseStream')).toBe(true);
    expect(source.includes('leadThinking')).toBe(true);
  });
});

describe('pet motion + merge', () => {
  test('keeps sleepy mood on sleep until a live job arrives', () => {
    expect(motionFromMood('sleepy', 'idle')).toBe('sleep');
    expect(resolveCompanionMotion('sleepy', 'idle', [])).toBe('sleep');
  });

  test('lets a running job override idle mood with orbit', () => {
    const running: PetMessage = {
      id: 'a',
      source: 'conversation',
      phase: 'running',
      titleKey: key('nomi.petMessage.conversation.running'),
      updatedAt: 2,
    };
    expect(resolveCompanionMotion('content', 'idle', [running])).toBe('orbit');
    expect(leadingPetPhase([running])).toBe('running');
  });

  test('replaces the same correlation id in place and prefers active work', () => {
    const first: PetMessage = {
      id: 'job',
      source: 'learning',
      phase: 'running',
      titleKey: key('nomi.petMessage.learning.course.round'),
      updatedAt: 1,
    };
    const done: PetMessage = { ...first, phase: 'completed', updatedAt: 2 };
    const other: PetMessage = {
      id: 'other',
      source: 'cron',
      phase: 'completed',
      titleKey: key('nomi.petMessage.cron.completed'),
      titleParams: { name: 'x' },
      updatedAt: 3,
    };
    const merged = mergePetMessage([first, other], done);
    expect(merged.find((m) => m.id === 'job')?.phase).toBe('completed');
    expect(merged.map((m) => m.id).toSorted()).toEqual(['job', 'other']);
    expect(mergePetMessage([other], first).map((m) => m.id)).toEqual(['job', 'other']);
  });

  test('truncates process detail without chopping a short string', () => {
    expect(truncateDetail('ok')).toBe('ok');
    expect(truncateDetail('x'.repeat(90)).length).toBe(80);
  });

  test('quiet hours drop an in-flight chip instead of leaving it running', () => {
    const running: PetMessage = {
      id: 'a',
      source: 'conversation',
      phase: 'running',
      titleKey: key('nomi.petMessage.conversation.running'),
      updatedAt: 2,
    };
    const folded = foldIncomingPetMessage([running], { ...running, phase: 'completed', updatedAt: 9 }, { quiet: true });
    expect(folded).toEqual([]);
    expect(foldIncomingPetMessage([], { ...running, phase: 'completed', updatedAt: 9 }, { quiet: true })).toEqual([]);
  });
});

describe('projectOrClear', () => {
  const idle: PetMessage = {
    id: 'ssh:host',
    source: 'ssh',
    phase: 'completed',
    titleKey: key('nomi.petMessage.ssh.idle'),
    updatedAt: 0,
  };
  const connecting: PetMessage = {
    ...idle,
    phase: 'running',
    titleKey: key('nomi.petMessage.ssh.connecting'),
    updatedAt: 1,
  };

  test('ignores a cold idle snapshot', () => {
    expect(projectOrClear(new Set(), null, idle)).toBeNull();
  });

  test('clears a live chip when the source goes idle', () => {
    const live = new Set<string>();
    expect(projectOrClear(live, connecting, idle)?.phase).toBe('running');
    expect(live.has('ssh:host')).toBe(true);
    const cleared = projectOrClear(live, null, idle);
    expect(cleared?.phase).toBe('completed');
    expect(cleared?.titleKey).toBe('nomi.petMessage.ssh.idle');
    expect(live.size).toBe(0);
  });

  test('does not re-emit idle after a terminal envelope already closed the job', () => {
    const live = new Set<string>();
    projectOrClear(live, connecting, idle);
    const connected = { ...idle, titleKey: key('nomi.petMessage.ssh.connected'), updatedAt: 2 };
    expect(projectOrClear(live, connected, idle)?.phase).toBe('completed');
    expect(projectOrClear(live, null, idle)).toBeNull();
  });

  test('autowork that is not running stays silent until it was live', () => {
    expect(
      petMessageFromAutoWork({
        kind: 'conversation',
        target_id: CID,
        enabled: true,
        running: false,
        run_state: 'idle',
        completed_count: 0,
      })
    ).toBeNull();
  });
});
