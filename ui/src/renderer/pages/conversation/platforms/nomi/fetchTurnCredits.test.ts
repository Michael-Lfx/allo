/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import type { TurnCreditUsageData } from '@/common/config/storage';
import {
  normalizeTurnCreditUsage,
  pickRicherTurnCredits,
  shouldReuseCachedTurnCredits,
  TURN_CREDIT_LATE_REFRESH_MS,
  TURN_CREDIT_SETTLE_MS,
  isFlowyCloudConversation,
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
  test('ordinary cloud eligibility depends only on the task model provider', () => {
    const local = { type: 'nomi', model: { id: '0190f5fe-7c00-7a00-8000-000000000901' }, extra: {} };
    const cloud = { ...local, model: { id: '0190f5fe-7c00-7a00-8000-000000000201' } };
    expect(isFlowyCloudConversation(local)).toBe(false);
    expect(isFlowyCloudConversation(cloud)).toBe(true);
    expect(isFlowyCloudConversation({ ...local, extra: { voice: true } })).toBe(false);
    expect(isFlowyCloudConversation({ ...cloud, extra: { voice: true } })).toBe(true);
    expect(isFlowyCloudConversation(null)).toBe(false);
  });

  test('the generic query module has no voice-specific dependency', () => {
    const source = readFileSync(new URL('./fetchTurnCredits.ts', import.meta.url), 'utf8');
    expect(source.includes('isVoiceConversation')).toBe(false);
    expect(source.includes('voiceCompanion')).toBe(false);
    expect(source.includes('kws_ready')).toBe(false);
    expect(source.includes('acoustic_ready')).toBe(false);
    expect(source.includes('barge_in_ready')).toBe(false);
    const messageText = readFileSync(new URL('../../Messages/components/MessageText.tsx', import.meta.url), 'utf8');
    expect(messageText.includes('isVoiceConversation')).toBe(false);
    expect(messageText.includes('kws_ready')).toBe(false);
    expect(messageText.includes('acoustic_ready')).toBe(false);
    expect(messageText.includes('barge_in_ready')).toBe(false);
  });
  test('reuses a positive snapshot unless force-refresh or settle is requested', () => {
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

  test('terminal fetches settle past a positive mid-turn snapshot', () => {
    const fetchSource = readFileSync(new URL('./fetchTurnCredits.ts', import.meta.url), 'utf8');
    const hookSource = readFileSync(new URL('./useNomiMessage.ts', import.meta.url), 'utf8');
    expect(fetchSource.includes('params.force === true || params.settle === true')).toBe(true);
    expect(fetchSource.includes('existingInflight && !params.force && !params.settle')).toBe(true);
    expect(hookSource.includes('settle: true')).toBe(true);
  });
});
