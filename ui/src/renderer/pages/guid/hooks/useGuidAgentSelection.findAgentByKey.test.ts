

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('useGuidAgentSelection findAgentByKey alias lookup', () => {
  test('resolves preset keys through findPresetById', () => {
    const source = readSource(new URL('./useGuidAgentSelection.ts', import.meta.url));

    expect(source.includes('parsePresetSelectionId(key)')).toBe(true);
    expect(source.includes('findPresetById(presets, presetId)')).toBe(true);
    expect(source.includes("presets.find((a) => a.id === presetId)")).toBe(false);
  });
});
