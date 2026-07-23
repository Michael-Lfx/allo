/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import {
  beginTurnTiming,
  confirmFirstValue,
  getFunnelCohort,
  hasFunnelEvent,
  listFunnelEvents,
  markTurnAccepted,
  markTurnFirstToken,
  markTurnIdle,
  markTurnStreamFinished,
  resetFunnelForTests,
  resetTurnTimingForTests,
  trackFunnelEvent,
} from './productFunnel';

describe('product funnel', () => {
  test('records auth and accepted first-task events with a stable cohort', () => {
    resetFunnelForTests();
    const cohort = getFunnelCohort();
    expect(cohort === 'A' || cohort === 'B').toBe(true);
    trackFunnelEvent('auth_completed');
    trackFunnelEvent('home_interactive');
    trackFunnelEvent('task_accepted', { source: 'guid' });
    trackFunnelEvent('first_task_started', { source: 'guid' });
    expect(hasFunnelEvent('auth_completed')).toBe(true);
    expect(hasFunnelEvent('task_accepted')).toBe(true);
    expect(hasFunnelEvent('first_task_started')).toBe(true);
    const last = listFunnelEvents().at(-1);
    expect(last?.cohort).toBe(cohort);
    expect(last?.props?.runtime === 'desktop' || last?.props?.runtime === 'webui').toBe(true);
    expect(last?.props?.viewport === 'desktop' || last?.props?.viewport === 'mobile').toBe(true);
    expect(last?.props?.first_win_stage === 'active' || last?.props?.first_win_stage === 'completed').toBe(true);
    expect(last?.props?.source).toBe('guid');
  });

  test('does not treat first token as first value', () => {
    resetFunnelForTests();
    resetTurnTimingForTests();
    beginTurnTiming('req-1', { conversation_type: 'nomi', cold_start: true });
    expect(markTurnAccepted('req-1')).not.toBeNull();
    expect(markTurnFirstToken('req-1')).not.toBeNull();
    expect(hasFunnelEvent('first_value_confirmed')).toBe(false);
    expect(markTurnStreamFinished('req-1')).not.toBeNull();
    expect(markTurnIdle('req-1', 'completed')).not.toBeNull();
    expect(hasFunnelEvent('answer_completed')).toBe(true);
    expect(hasFunnelEvent('first_artifact_visible')).toBe(true);
    expect(hasFunnelEvent('first_value_confirmed')).toBe(false);
    expect(confirmFirstValue({ source: 'follow_up' })).not.toBeNull();
    expect(hasFunnelEvent('first_value_confirmed')).toBe(true);
  });
});
