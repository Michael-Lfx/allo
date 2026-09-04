import { parsePresetId, type PresetId } from '@/common/types/ids';

// This mapping intentionally survives ranking-cache revisions. It is the
// only local evidence that a user preset originated from the market, so a
// missing mapping must never be reconstructed from a matching name.
export const PRESET_MARKET_ID_STORAGE_KEY = 'nomifun.presetMarket.itemPresetIds.v1';

export type PresetMarketStorage = Pick<Storage, 'getItem' | 'setItem'> &
  Partial<Pick<Storage, 'removeItem'>>;

export const presetMarketStorage = (): PresetMarketStorage | null => {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
};

export const readPresetMarketIds = (
  storage: PresetMarketStorage | null = presetMarketStorage(),
): Record<string, PresetId> => {
  if (!storage) return {};
  try {
    const parsed = JSON.parse(storage.getItem(PRESET_MARKET_ID_STORAGE_KEY) || '{}') as Record<string, unknown>;
    return Object.fromEntries(
      Object.entries(parsed).flatMap(([itemId, value]) => {
        try {
          return [[itemId, parsePresetId(value)]];
        } catch {
          return [];
        }
      }),
    );
  } catch {
    return {};
  }
};

export const removePresetMarketId = (
  itemId: string,
  storage: PresetMarketStorage | null = presetMarketStorage(),
  expectedPresetId?: PresetId,
): void => {
  if (!storage) return;
  const ids = readPresetMarketIds(storage);
  if (expectedPresetId && ids[itemId] !== expectedPresetId) return;
  if (!(itemId in ids)) return;
  delete ids[itemId];
  try {
    storage.setItem(PRESET_MARKET_ID_STORAGE_KEY, JSON.stringify(ids));
  } catch {
    // The mapping is only a duplicate guard; the preset API is authoritative.
  }
};

/** Remove every stale market mapping that points at a deleted user preset. */
export const removePresetMarketIdsForPreset = (
  presetId: PresetId,
  storage: PresetMarketStorage | null = presetMarketStorage(),
): void => {
  if (!storage) return;
  const ids = readPresetMarketIds(storage);
  const next = Object.fromEntries(Object.entries(ids).filter(([, mappedPresetId]) => mappedPresetId !== presetId));
  if (Object.keys(next).length === Object.keys(ids).length) return;
  try {
    storage.setItem(PRESET_MARKET_ID_STORAGE_KEY, JSON.stringify(next));
  } catch {
    // The mapping is auxiliary; the preset API remains authoritative.
  }
};
