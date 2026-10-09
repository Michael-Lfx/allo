

import { describe, expect, test } from 'bun:test';
import { CHARACTERS, DEFAULT_CHARACTER_ID, DEFAULT_DESK, getCharacter, getDeskSpec } from './index';

describe('getDeskSpec', () => {
  test('falls back to DEFAULT_DESK for unknown / missing ids', () => {
    expect(getDeskSpec('no-such-character')).toBe(DEFAULT_DESK);
    expect(getDeskSpec(null)).toBe(DEFAULT_DESK);
    expect(getDeskSpec(undefined)).toBe(DEFAULT_DESK);
  });

  test('keeps every roster character on the default desk', () => {
    for (const id of ['puff', 'mochi', 'ink', 'bolt']) {
      expect(getDeskSpec(id)).toBe(DEFAULT_DESK);
    }
  });

  test('offers Puff first for new companions while unknown ids stay on Mochi', () => {
    expect(DEFAULT_CHARACTER_ID).toBe('puff');
    expect(CHARACTERS[0]?.id).toBe('puff');
    expect(getCharacter(null).id).toBe('mochi');
    expect(getCharacter('no-such-character').id).toBe('mochi');
    expect(getCharacter('puff').id).toBe('puff');
  });

  test('uses the teal palette for bolt', () => {
    const bolt = CHARACTERS.find((c) => c.id === 'bolt');
    expect(bolt?.palette).toEqual(['#bfeee0', '#37e0ff']);
  });
});
