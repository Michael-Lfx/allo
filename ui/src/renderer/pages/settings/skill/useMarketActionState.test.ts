import { describe, expect, test } from 'bun:test';
import { mergeMarketActionState } from './useMarketActionState';

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
});
