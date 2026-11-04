import { describe, expect, test } from 'bun:test';
import {
  filterAllowedImageModels,
  pickDefaultLlmModel,
  pickDefaultVideoModel,
} from './modelPreferenceDefaults';

describe('modelPreferenceDefaults', () => {
  test('prefers Wan 3.0 over other video models, and the non-prime id when both exist', () => {
    expect(
      pickDefaultVideoModel([
        { id: 'AIPC-doubao-seedance-2-0-fast', name: 'Doubao-Seedance-2.0-Fast' },
        { id: 'AIPC-wan3.0-video-prime', name: 'Wan 3.0 Prime' },
        { id: 'flowy/wan3.0-video', name: 'Wan 3.0' },
      ])
    ).toBe('flowy/wan3.0-video');
  });

  test('falls back to Wan 3.0 prime, then the first catalog model', () => {
    expect(
      pickDefaultVideoModel([
        { id: 'other', name: 'Other' },
        { id: 'AIPC-wan3.0-video-prime', name: 'Wan 3.0 Prime' },
      ])
    ).toBe('AIPC-wan3.0-video-prime');
    expect(
      pickDefaultVideoModel([{ id: 'AIPC-doubao-seedance-2-0-fast', name: 'Seedance' }])
    ).toBe('AIPC-doubao-seedance-2-0-fast');
  });

  test('filters Seedream 5.0 Lite and Pro image models', () => {
    const allowed = filterAllowedImageModels([
      { id: 'a', name: 'Doubao-seedream-5-0-lite' },
      { id: 'b', name: 'Other' },
      { id: 'c', name: 'Doubao-seedream-5-0-pro' },
    ]);
    expect(allowed.map((m) => m.id)).toEqual(['a', 'c']);
  });

  test('prefers Deepseek-v4-pro for planning', () => {
    expect(pickDefaultLlmModel(['gpt-x', 'Deepseek-v4-pro'])).toBe('Deepseek-v4-pro');
  });
});
