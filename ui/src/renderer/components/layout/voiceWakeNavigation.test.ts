/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';

import {
  isVoiceSessionInactive,
  nextVoiceWakeNavigation,
  shouldFollowVoiceSession,
  type VoiceSessionRouteSnapshot,
} from './voiceWakeNavigation';

function snapshot(overrides: Partial<VoiceSessionRouteSnapshot> = {}): VoiceSessionRouteSnapshot {
  return {
    enabled: true,
    connection: 'connected',
    phase: 'listening',
    self_check_active: false,
    session_id: 'session-1',
    binding: { conversation_id: 'conv-voice' },
    ...overrides,
  };
}

describe('voice wake navigation (problem 1: stop stealing the route)', () => {
  test('navigates on a new call activation', () => {
    expect(shouldFollowVoiceSession(null, snapshot())).toBe('/conversation/conv-voice');
    expect(shouldFollowVoiceSession('previous-session', snapshot())).toBe('/conversation/conv-voice');
  });

  test('does not navigate again for the same active session (user switched away)', () => {
    expect(shouldFollowVoiceSession('session-1', snapshot())).toBeNull();
    expect(shouldFollowVoiceSession('session-1', snapshot({ phase: 'thinking' }))).toBeNull();
  });

  test('never navigates during self check', () => {
    expect(shouldFollowVoiceSession(null, snapshot({ self_check_active: true }))).toBeNull();
  });

  test('does not navigate when the call is inactive', () => {
    for (const phase of ['idle', 'error', '', null]) {
      expect(shouldFollowVoiceSession(null, snapshot({ phase }))).toBeNull();
    }
    expect(shouldFollowVoiceSession(null, snapshot({ connection: 'connecting' }))).toBeNull();
    expect(shouldFollowVoiceSession(null, snapshot({ enabled: false }))).toBeNull();
    expect(shouldFollowVoiceSession(null, snapshot({ binding: null }))).toBeNull();
  });

  test('an unbound active frame keeps the activation available for the later bound frame', () => {
    const unbound = nextVoiceWakeNavigation(null, snapshot({ binding: null }), '/guid');
    expect(unbound).toEqual({ followedSessionId: null, target: null });

    const bound = nextVoiceWakeNavigation(unbound.followedSessionId, snapshot(), '/guid');
    expect(bound.target).toBe('/conversation/conv-voice');
    expect(bound.followedSessionId).toBe('session-1');

    // The same consumed session, and arriving on the target page, must not navigate again.
    expect(nextVoiceWakeNavigation(bound.followedSessionId, snapshot(), '/guid').target).toBeNull();
    expect(
      nextVoiceWakeNavigation(null, snapshot(), '/conversation/conv-voice')
    ).toEqual({ followedSessionId: 'session-1', target: null });
  });

  test('a stale unbound frame does not clear an older consumed session', () => {
    expect(nextVoiceWakeNavigation('session-1', snapshot({ binding: null }), '/guid')).toEqual({
      followedSessionId: 'session-1',
      target: null,
    });
  });

  test('idle snapshots reset the followed session so the next wake can navigate', () => {
    expect(isVoiceSessionInactive(snapshot({ phase: 'idle' }))).toBe(true);
    expect(isVoiceSessionInactive(snapshot({ phase: 'error' }))).toBe(true);
    expect(isVoiceSessionInactive(snapshot({ self_check_active: true }))).toBe(true);
    expect(isVoiceSessionInactive(snapshot({ phase: 'speaking' }))).toBe(false);
  });
});
