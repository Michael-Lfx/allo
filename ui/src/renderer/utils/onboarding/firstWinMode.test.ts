/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { resetFunnelForTests, trackFunnelEvent } from '../analytics/productFunnel';
import {
  isFirstWinCompleted,
  markFirstWinCompleted,
  resetFirstWinForTests,
} from './firstWinMode';

describe('firstWinMode', () => {
  test('starts incomplete and becomes complete after mark', () => {
    resetFunnelForTests();
    resetFirstWinForTests();
    expect(isFirstWinCompleted()).toBe(false);
    markFirstWinCompleted();
    expect(isFirstWinCompleted()).toBe(true);
  });

  test('treats answer_completed as first-win completion', () => {
    resetFunnelForTests();
    resetFirstWinForTests();
    trackFunnelEvent('answer_completed', { source: 'chat' });
    expect(isFirstWinCompleted()).toBe(true);
  });
});
