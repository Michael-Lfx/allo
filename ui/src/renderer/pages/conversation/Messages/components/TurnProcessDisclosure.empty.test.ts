/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('TurnProcessDisclosure empty process items', () => {
  test('renders static label instead of a disabled toggle when there are no process items', () => {
    const source = readSource(new URL('./TurnProcessDisclosure.tsx', import.meta.url));

    expect(source.includes('!hasProcessItems ? (')).toBe(true);
    expect(source.includes('turn-process-disclosure__label--static')).toBe(true);
  });
});
