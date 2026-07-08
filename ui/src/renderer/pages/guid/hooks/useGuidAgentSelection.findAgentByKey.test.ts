/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('useGuidAgentSelection findAgentByKey alias lookup', () => {
  test('resolves custom preset keys through findAssistantById', () => {
    const source = readSource(new URL('./useGuidAgentSelection.ts', import.meta.url));

    expect(source.includes('findAssistantById(assistants, assistantId)')).toBe(true);
    expect(source.includes('toPresetAvailableAgent(assistant)')).toBe(true);
    expect(source.includes('assistants.find((a) => a.id === assistantId)')).toBe(false);
  });
});
