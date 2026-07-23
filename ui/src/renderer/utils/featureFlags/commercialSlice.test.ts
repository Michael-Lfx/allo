

import { describe, expect, test } from 'bun:test';
import { COMMERCIAL_SLICE_FLAG, isCommercialSliceEnabled, readCommercialSliceEnabled } from './commercialSlice';

describe('commercial slice flag', () => {
  test('defaults on and exposes a stable storage key', () => {
    expect(COMMERCIAL_SLICE_FLAG).toBe('flowy.commercialSlice.v1');
    expect(isCommercialSliceEnabled()).toBe(true);
    expect(readCommercialSliceEnabled().source === 'default' || readCommercialSliceEnabled().source === 'localStorage').toBe(true);
  });
});
