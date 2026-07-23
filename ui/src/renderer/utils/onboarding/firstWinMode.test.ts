

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

  test('does not treat answer_completed as first-win completion', () => {
    resetFunnelForTests();
    resetFirstWinForTests();
    trackFunnelEvent('answer_completed', { source: 'chat' });
    trackFunnelEvent('first_artifact_visible', { source: 'answer' });
    expect(isFirstWinCompleted()).toBe(false);
    trackFunnelEvent('first_value_confirmed', { source: 'outcome_confirm' });
    expect(isFirstWinCompleted()).toBe(true);
  });
});
