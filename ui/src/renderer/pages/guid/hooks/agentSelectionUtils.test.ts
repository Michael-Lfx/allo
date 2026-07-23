

import { describe, expect, test } from 'bun:test';
import {
  findPresetById,
  parsePresetSelectionId,
  presetIdMatches,
} from './agentSelectionUtils';

describe('agentSelectionUtils preset selection helpers', () => {
  test('parsePresetSelectionId reads preset selection keys', () => {
    expect(parsePresetSelectionId('preset:abc')).toBe('abc');
    expect(parsePresetSelectionId('nomi')).toBeNull();
    expect(parsePresetSelectionId('preset:')).toBeNull();
    expect(parsePresetSelectionId('custom:abc')).toBeNull();
  });

  test('presetIdMatches normalizes builtin aliases', () => {
    expect(presetIdMatches('builtin-cowork', 'cowork')).toBe(true);
    expect(presetIdMatches('cowork', 'builtin-cowork')).toBe(true);
    expect(presetIdMatches('other', 'cowork')).toBe(false);
  });

  test('findPresetById resolves alias ids in the catalog', () => {
    const presets = [{ id: 'builtin-cowork', name: 'Cowork' }];
    expect(findPresetById(presets, 'cowork')?.name).toBe('Cowork');
  });
});
