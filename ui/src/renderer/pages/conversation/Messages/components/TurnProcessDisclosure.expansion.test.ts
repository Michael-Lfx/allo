import { describe, expect, test } from 'bun:test';

import {
  shouldResetTurnProcessDisclosureExpansion,
  stabilizeTurnProcessDisclosureKeys,
} from './TurnProcessDisclosure';

describe('TurnProcessDisclosure expansion state', () => {
  test('does not reset the same turn when only defaultCollapsed changes', () => {
    expect(
      shouldResetTurnProcessDisclosureExpansion(
        { itemId: 'turn-disclosure-1', hasProcessItems: true },
        { itemId: 'turn-disclosure-1', hasProcessItems: true }
      )
    ).toBe(false);
  });

  test('resets when a new turn disclosure replaces the current one', () => {
    expect(
      shouldResetTurnProcessDisclosureExpansion(
        { itemId: 'turn-disclosure-1', hasProcessItems: true },
        { itemId: 'turn-disclosure-2', hasProcessItems: true }
      )
    ).toBe(true);
  });

  test('resets when process items first arrive for the current turn', () => {
    expect(
      shouldResetTurnProcessDisclosureExpansion(
        { itemId: 'turn-disclosure-1', hasProcessItems: false },
        { itemId: 'turn-disclosure-1', hasProcessItems: true }
      )
    ).toBe(true);
  });

  test('reuses the previous keys array when membership is unchanged', () => {
    const previous = ['thinking-1', 'thinking-2'];
    const next = ['thinking-1', 'thinking-2'];
    expect(stabilizeTurnProcessDisclosureKeys(previous, next)).toBe(previous);
  });

  test('returns the next keys array when membership changes', () => {
    const previous = ['thinking-1'];
    const next = ['thinking-1', 'thinking-2'];
    expect(stabilizeTurnProcessDisclosureKeys(previous, next)).toBe(next);
  });
});
