import { describe, expect, test } from 'bun:test';
import { canRunMarketAction, mergeMarketActionState, shouldReleaseHiddenMarketAction } from './useMarketActionState';

describe('market action state', () => {
  test('merges a shared pending item without changing authoritative completion state', () => {
    expect(mergeMarketActionState('ready', 'skill-1', null)).toBe('ready');
    expect(mergeMarketActionState('completed', 'skill-1', null)).toBe('completed');
    expect(mergeMarketActionState('ready', 'skill-1', 'skill-1')).toBe('pending');
    expect(mergeMarketActionState('completed', 'skill-1', 'skill-2')).toBe('completed');
  });

  test('keeps error retryable while pending and completed remain guarded', () => {
    expect(mergeMarketActionState('error', 'skill-1', null)).toBe('error');
    expect(mergeMarketActionState('ready', 'skill-1', 'skill-1')).toBe('pending');
    expect(mergeMarketActionState('completed', 'skill-1', null)).toBe('completed');
  });

  test('allows a completed action when the consumer explicitly makes it repeatable', () => {
    expect(canRunMarketAction('completed', true)).toBe(true);
    expect(canRunMarketAction('completed', false)).toBe(false);
    expect(canRunMarketAction('checking', true)).toBe(false);
  });

  test('releases an in-flight action once its card is no longer on screen', () => {
    const visible = new Set(['skillhub:indiv-integrator/skills/dev-expert-pro']);
    expect(shouldReleaseHiddenMarketAction('skillhub:user_814dbe54/skills/dev-expert-pro', visible)).toBe(true);
    expect(shouldReleaseHiddenMarketAction('skillhub:indiv-integrator/skills/dev-expert-pro', visible)).toBe(false);
    expect(shouldReleaseHiddenMarketAction(null, visible)).toBe(false);
  });
});
