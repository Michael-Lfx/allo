/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import {
  beginTurnTiming,
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
  test('records auth and first-task events with a stable cohort', () => {
    resetFunnelForTests();
    const cohort = getFunnelCohort();
    expect(cohort === 'A' || cohort === 'B').toBe(true);
    trackFunnelEvent('auth_completed');
    trackFunnelEvent('first_task_started', { source: 'guid' });
    expect(hasFunnelEvent('auth_completed')).toBe(true);
    expect(hasFunnelEvent('first_task_started')).toBe(true);
    expect(listFunnelEvents().at(-1)?.cohort).toBe(cohort);
  });

  test('records turn timing milestones and finalization gap', () => {
    resetFunnelForTests();
    resetTurnTimingForTests();
    beginTurnTiming('req-1', { conversation_type: 'nomi', cold_start: true });
    expect(markTurnAccepted('req-1')).not.toBeNull();
    expect(markTurnFirstToken('req-1')).not.toBeNull();
    expect(hasFunnelEvent('first_value_confirmed')).toBe(true);
    expect(markTurnStreamFinished('req-1')).not.toBeNull();
    expect(markTurnIdle('req-1', 'completed')).not.toBeNull();
    const names = listFunnelEvents().map((event) => event.name);
    expect(names).toContain('message_submitted');
    expect(names).toContain('message_accepted');
    expect(names).toContain('first_token');
    expect(names).toContain('stream_finished');
    expect(names).toContain('turn_idle');
  });
});
