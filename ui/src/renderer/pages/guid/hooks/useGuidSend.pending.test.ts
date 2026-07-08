/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('useGuidSend pending preset guard', () => {
  test('blocks send and disables the button until the preset catalog resolves', () => {
    const source = readSource(new URL('./useGuidSend.ts', import.meta.url));

    expect(source.includes('if (is_preset && !agentInfo)')).toBe(true);
    expect(source.includes('is_presetAgentPending && !resolvedPresetSelection')).toBe(true);
  });
});
