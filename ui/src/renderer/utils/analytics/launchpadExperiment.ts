/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { getFunnelCohort } from '@renderer/utils/analytics/productFunnel';

export type LaunchpadExperimentVariant = 'control' | 'launchpad';

const STORAGE_KEY = 'flowy.experiment.launchpad.v1';

let memoryVariant: LaunchpadExperimentVariant | null = null;

function canUseStorage(): boolean {
  return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined';
}

/**
 * Phase-1 A/B: cohort A → launchpad treatment, cohort B → control labeling.
 * Product behavior for launchpad is already shipped; this flag is for analytics
 * segmentation and staged rollout gates in study reports.
 */
export function getLaunchpadExperimentVariant(): LaunchpadExperimentVariant {
  if (memoryVariant) return memoryVariant;
  if (canUseStorage()) {
    try {
      const existing = window.localStorage.getItem(STORAGE_KEY);
      if (existing === 'control' || existing === 'launchpad') {
        memoryVariant = existing;
        return existing;
      }
    } catch {
      // fall through
    }
  }
  const cohort = getFunnelCohort();
  const next: LaunchpadExperimentVariant = cohort === 'A' ? 'launchpad' : 'control';
  memoryVariant = next;
  if (canUseStorage()) {
    try {
      window.localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // ignore
    }
  }
  return next;
}

export function isLaunchpadTreatment(): boolean {
  return getLaunchpadExperimentVariant() === 'launchpad';
}

export function resetLaunchpadExperimentForTests(): void {
  memoryVariant = null;
  if (!canUseStorage()) return;
  try {
    window.localStorage.removeItem(STORAGE_KEY);
  } catch {
    // ignore
  }
}
