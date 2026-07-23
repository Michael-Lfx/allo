/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import {
  getTurnStatusLabel,
  initialTurnPresentationState,
  isTurnPresentationActive,
  turnPresentationReducer,
} from './turnPresentationState';

describe('turnPresentationReducer', () => {
  test('moves local submit into a stoppable pending phase', () => {
    const next = turnPresentationReducer(initialTurnPresentationState, {
      type: 'localSubmit',
      localRequestId: 'local-1',
      at: 1000,
    });
    expect(next.phase).toBe('local_pending');
    expect(next.showStop).toBe(true);
    expect(next.showStatusRail).toBe(true);
    expect(next.composerInteractive).toBe(false);
    expect(isTurnPresentationActive(next)).toBe(true);
  });

  test('keeps composer interactive while finalizing after stream finish', () => {
    let state = turnPresentationReducer(initialTurnPresentationState, {
      type: 'localSubmit',
      localRequestId: 'local-1',
    });
    state = turnPresentationReducer(state, { type: 'streaming', at: 1100 });
    state = turnPresentationReducer(state, { type: 'streamFinished', at: 1200 });
    expect(state.phase).toBe('finalizing');
    expect(state.showStop).toBe(false);
    expect(state.composerInteractive).toBe(true);
    expect(state.showStatusRail).toBe(true);
  });

  test('maps wire turnStarted phases and settles on turnCompleted', () => {
    let state = turnPresentationReducer(initialTurnPresentationState, {
      type: 'turnStarted',
      turnId: 'turn-1' as never,
      phase: 'starting',
    });
    expect(state.phase).toBe('preparing');
    state = turnPresentationReducer(state, {
      type: 'waitingPermission',
      detail: 'Allow edit',
    });
    expect(state.phase).toBe('waiting_permission');
    expect(state.composerInteractive).toBe(true);
    state = turnPresentationReducer(state, { type: 'turnCompleted', at: 2000 });
    expect(state.phase).toBe('completed');
    expect(state.activeTurnId).toBeUndefined();
    expect(isTurnPresentationActive(state)).toBe(false);
  });

  test('status labels fall back to detail when present', () => {
    expect(getTurnStatusLabel('tooling', 'Reading auth.ts')).toBe('Reading auth.ts');
    expect(getTurnStatusLabel('finalizing')).toContain('Saving');
  });
});
