import { describe, expect, test } from 'bun:test';
import {
  filterAllowedImageModels,
  pickDefaultLlmModel,
  pickDefaultVideoModel,
} from './modelPreferenceDefaults';

describe('modelPreferenceDefaults', () => {
  test('prefers Seedance 2.0 Fast when present', () => {
    expect(
      pickDefaultVideoModel([
        { id: 'other', name: 'Other' },
        { id: 'AIPC-doubao-seedance-2-0-fast', name: 'Doubao-Seedance-2.0-Fast' },
      ])
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
