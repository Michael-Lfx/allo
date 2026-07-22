/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { getFunnelCohort, hasFunnelEvent, listFunnelEvents, resetFunnelForTests, trackFunnelEvent } from './productFunnel';

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
});
