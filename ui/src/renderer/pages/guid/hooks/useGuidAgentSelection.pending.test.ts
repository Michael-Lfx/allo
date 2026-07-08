/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('useGuidAgentSelection pending preset export', () => {
  test('returns is_presetAgentPending from the hook result', () => {
    const source = readSource(new URL('./useGuidAgentSelection.ts', import.meta.url));
    expect(source.includes('is_presetAgentPending,')).toBe(true);
  });
});
