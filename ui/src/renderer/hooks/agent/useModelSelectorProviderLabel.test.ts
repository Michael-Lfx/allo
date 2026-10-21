import { describe, expect, test } from 'bun:test';

import { HERDSMAN_PLATFORM } from '@/common/types/provider/herdsman';
import { NOMIFUN_FREE_MODEL_PLATFORM } from '@/common/types/provider/managedModelService';
import { formatModelSelectorProviderLabel } from './useModelSelectorProviderLabel';

const labels = {
  free: '免费模型',
  herdsman: '牧马人',
};

describe('formatModelSelectorProviderLabel', () => {
  test('localizes the managed free provider platform', () => {
    expect(formatModelSelectorProviderLabel({ platform: NOMIFUN_FREE_MODEL_PLATFORM }, labels)).toBe('免费模型');
  });

  test('localizes the Herdsman engine instead of its stored name', () => {
    expect(
      formatModelSelectorProviderLabel({ name: 'Herdsman', platform: HERDSMAN_PLATFORM }, labels),
    ).toBe('牧马人');
  });

  test('preserves ordinary provider names and falls back to the platform', () => {
    expect(formatModelSelectorProviderLabel({ name: 'OpenAI', platform: 'openai' }, labels)).toBe('OpenAI');
    expect(formatModelSelectorProviderLabel({ platform: 'anthropic' }, labels)).toBe('anthropic');
  });
});
