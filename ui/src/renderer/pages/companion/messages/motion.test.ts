import { describe, expect, it } from 'vitest';
import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';
import { motionFromVoicePhase, resolveCompanionMotion } from './motion';
import type { PetMessage } from './types';

describe('Voice phase posture resolution and coexistence (R07)', () => {
  const baseVoice = (
    phase: string,
    endedReason: VoiceSnapshot['ended_reason'] = null
  ): VoiceSnapshot => ({
    instance_id: 'inst-1',
    seq: 1,
    session_id: 'session-1',
    enabled: true,
    phase: phase as any,
    turn_id: 'turn-1',
    user_text: null,
    assistant_text: null,
    dialogue_ready: true,
    playback_ready: true,
    ended_reason: endedReason,
    error: null,
    connection: 'connected',
    acoustic_ready: true,
    binding: {
      session_id: 'session-1',
      binding_id: 'b-1',
      conversation_id: 'conv-1',
      workspace: null,
      memory_owner_companion_id: 'flowy-bolt',
    },
  });

  it('maps acoustic listening and follow_up phases to alert motion (Bolt listening posture)', () => {
    expect(motionFromVoicePhase(baseVoice('listening'), 'flowy-bolt')).toBe('alert');
    expect(motionFromVoicePhase(baseVoice('follow_up'), 'flowy-bolt')).toBe('alert');
  });

  it('maps deciding and transcribing phases to thinking motion', () => {
    expect(motionFromVoicePhase(baseVoice('deciding'), 'flowy-bolt')).toBe('thinking');
    expect(motionFromVoicePhase(baseVoice('transcribing'), 'flowy-bolt')).toBe('thinking');
  });

  it('maps synthesizing and speaking phases to notify motion (Bolt speaking posture)', () => {
    expect(motionFromVoicePhase(baseVoice('synthesizing'), 'flowy-bolt')).toBe('notify');
    expect(motionFromVoicePhase(baseVoice('speaking'), 'flowy-bolt')).toBe('notify');
  });

  it('returns null when voice is idle, ended, unowned, or owner missing (F5)', () => {
    expect(motionFromVoicePhase(baseVoice('idle'), 'flowy-bolt')).toBeNull();
    expect(motionFromVoicePhase(baseVoice('speaking', 'user_stop'), 'flowy-bolt')).toBeNull();
    expect(motionFromVoicePhase(baseVoice('speaking'), 'other-pet')).toBeNull();

    // Missing or null owner in binding must NOT allow arbitrary companions to take motion
    const unownedVoice = {
      ...baseVoice('listening'),
      binding: {
        session_id: 'session-1',
        binding_id: 'b-1',
        conversation_id: 'conv-1',
        workspace: null,
        memory_owner_companion_id: null,
      },
    };
    expect(motionFromVoicePhase(unownedVoice, 'flowy-bolt')).toBeNull();
    expect(motionFromVoicePhase(unownedVoice, 'pet-b')).toBeNull();
  });

  it('preserves generic task semantics and coexistence with voice call posture', () => {
    const taskCompletedMessage: PetMessage = {
      id: 'task-1',
      source: 'execution',
      phase: 'completed',
      titleKey: 'voice.task.status.completed' as any,
      updatedAt: Date.now(),
    };

    // Generic task completed returns notify (celebration toast)
    const taskMotion = resolveCompanionMotion('content', 'idle', [taskCompletedMessage]);
    expect(taskMotion).toBe('notify');

    // While voice is live in listening, voice phase posture (alert) overrides task posture
    const voiceListening = baseVoice('listening');
    const overlayMotion = motionFromVoicePhase(voiceListening, 'flowy-bolt') || taskMotion;
    expect(overlayMotion).toBe('alert');

    // When voice ends, normal task motion takes back over
    const voiceIdle = baseVoice('idle');
    const restoredMotion = motionFromVoicePhase(voiceIdle, 'flowy-bolt') || taskMotion;
    expect(restoredMotion).toBe('notify');
  });
});
