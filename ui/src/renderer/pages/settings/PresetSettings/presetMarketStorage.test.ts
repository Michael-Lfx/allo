import { describe, expect, test } from 'bun:test';
import { parsePresetId } from '@/common/types/ids';
import {
  PRESET_MARKET_ID_STORAGE_KEY,
  readPresetMarketIds,
  removePresetMarketIdsForPreset,
  type PresetMarketStorage,
} from './presetMarketStorage';

const presetA = parsePresetId('0190f5fe-7c00-7a00-8000-000000000001');
const presetB = parsePresetId('0190f5fe-7c00-7a00-8000-000000000002');

const createStorage = (initial: Record<string, unknown>): PresetMarketStorage => {
  let value = JSON.stringify(initial);
  return {
    getItem: (key) => (key === PRESET_MARKET_ID_STORAGE_KEY ? value : null),
    setItem: (key, next) => {
      if (key === PRESET_MARKET_ID_STORAGE_KEY) value = next;
    },
  };
};

describe('preset market mapping cleanup', () => {
  test('removes every mapping for a deleted preset without guessing by name', () => {
    const storage = createStorage({
      packageA: presetA,
      packageAAlias: presetA,
      packageB: presetB,
    });

    removePresetMarketIdsForPreset(presetA, storage);

    expect(readPresetMarketIds(storage)).toEqual({ packageB: presetB });
  });

  test('ignores malformed mapping values while preserving valid entries', () => {
    const storage = createStorage({
      valid: presetA,
      malformed: 'not-a-uuid',
    });

    expect(readPresetMarketIds(storage)).toEqual({ valid: presetA });
  });
});
