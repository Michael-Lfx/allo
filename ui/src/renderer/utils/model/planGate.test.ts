/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { isMediaPlanLocked, isPlanLocked } from './planGate';

describe('plan name fallback', () => {
  test('keeps a model open when the numeric tier is already high enough', () => {
    expect(isPlanLocked(1, 2)).toBe(false);
    expect(
      isPlanLocked(1, 2, { plan: 'Free', planCode: 'FreePlan', requiredPlanName: 'Pro', requiredPlanCode: 'ProPlan' })
    ).toBe(false);
  });

  test('locks on the number when the member plan cannot be read', () => {
    expect(isPlanLocked(2, 0, { requiredPlanName: 'Pro', requiredPlanCode: 'ProPlan' })).toBe(true);
  });

  test('lets an Ultra member select a Pro model when the numeric tier is missing or too low', () => {
    const seedance = { min_tier_level: 5, required_plan_name: 'Pro', required_plan_code: 'ProPlan' };
    expect(isMediaPlanLocked(seedance, 0, { plan: 'Ultra', planCode: 'UltraPlan' })).toBe(false);
    expect(isPlanLocked(3, 0, { plan: 'Ultra', requiredPlanName: 'Pro' })).toBe(false);
    expect(isPlanLocked(3, 1, { planCode: 'UltraPlan', requiredPlanCode: 'ProPlan' })).toBe(false);
  });

  test('lets Ultra select a model that itself requires Ultra', () => {
    expect(isPlanLocked(3, 0, { plan: 'Ultra', planCode: 'UltraPlan', requiredPlanName: 'Ultra' })).toBe(false);
  });

  test('does not let Pro select a model that requires Ultra', () => {
    expect(isPlanLocked(3, 0, { plan: 'Pro', planCode: 'ProPlan', requiredPlanName: 'Ultra', requiredPlanCode: 'UltraPlan' })).toBe(
      true
    );
  });

  test('does not let Free select a Pro model', () => {
    expect(isPlanLocked(1, 0, { plan: 'Free', requiredPlanName: 'Pro' })).toBe(true);
  });

  test('ignores a plan name it does not recognize', () => {
    expect(isPlanLocked(2, 0, { plan: 'Studio', requiredPlanName: 'Pro' })).toBe(true);
    expect(isPlanLocked(2, 0, { plan: 'Ultra', requiredPlanName: 'Studio' })).toBe(true);
  });

  test('treats max as Ultra', () => {
    expect(isPlanLocked(2, 0, { plan: 'max', requiredPlanName: 'Pro' })).toBe(false);
  });
});
