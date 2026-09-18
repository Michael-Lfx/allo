/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import type { TurnCreditUsageData } from '@/common/config/storage';
import {
  normalizeTurnCreditUsage,
  pickRicherTurnCredits,
  shouldReuseCachedTurnCredits,
  TURN_CREDIT_LATE_REFRESH_MS,
  TURN_CREDIT_SETTLE_MS,
} from './fetchTurnCredits';

const usage = (creditsConsumed: number, callCount: number): TurnCreditUsageData => ({
  turnId: 'turn-1',
  creditsConsumed,
  callCount,
  calls: Array.from({ length: callCount }, (_, index) => ({
    modelName: 'model',
    creditConsumed: creditsConsumed / Math.max(callCount, 1),
  })),
});

describe('turn credit cache', () => {
  test('reuses a positive snapshot unless force-refresh is requested', () => {
    expect(shouldReuseCachedTurnCredits(usage(250, 1), false)).toBe(true);
    expect(shouldReuseCachedTurnCredits(usage(250, 1), true)).toBe(false);
    expect(shouldReuseCachedTurnCredits(usage(0, 0), false)).toBe(false);
  });

  test('keeps the richer snapshot so late write-back billing is not overwritten', () => {
    expect(pickRicherTurnCredits(usage(250, 1), usage(680, 2))).toEqual(usage(680, 2));
    expect(pickRicherTurnCredits(usage(680, 2), usage(250, 1))).toEqual(usage(680, 2));
    expect(pickRicherTurnCredits(null, usage(250, 1))).toEqual(usage(250, 1));
  });

  test('follows write-back billing with bounded late refreshes', () => {
    expect(TURN_CREDIT_LATE_REFRESH_MS).toEqual([2500, 8000, 22000]);
    expect(TURN_CREDIT_SETTLE_MS).toEqual([1500, 2500, 4000, 8000]);
  });

  test('raises aggregate credits to the per-call sum', () => {
    expect(
      normalizeTurnCreditUsage({
        turnId: 'turn-1',
        creditsConsumed: 10,
        callCount: 1,
        calls: [
          { modelName: 'm', creditConsumed: 40 },
          { modelName: 'm', creditConsumed: 30 },
        ],
      })
    ).toEqual({
      turnId: 'turn-1',
      creditsConsumed: 70,
      callCount: 2,
      calls: [
        { modelName: 'm', creditConsumed: 40 },
        { modelName: 'm', creditConsumed: 30 },
      ],
    });
  });
});
