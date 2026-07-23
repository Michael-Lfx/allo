/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { resetFunnelForTests } from './productFunnel';
import {
  getLaunchpadExperimentVariant,
  isLaunchpadTreatment,
  resetLaunchpadExperimentForTests,
} from './launchpadExperiment';

describe('launchpad experiment', () => {
  test('assigns a stable variant from the funnel cohort', () => {
    resetFunnelForTests();
    resetLaunchpadExperimentForTests();
    const first = getLaunchpadExperimentVariant();
    const second = getLaunchpadExperimentVariant();
    expect(first === 'control' || first === 'launchpad').toBe(true);
    expect(second).toBe(first);
    expect(isLaunchpadTreatment()).toBe(first === 'launchpad');
  });
});
