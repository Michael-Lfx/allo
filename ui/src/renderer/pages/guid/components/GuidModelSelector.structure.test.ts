/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./GuidModelSelector.tsx', import.meta.url), 'utf8');

describe('GuidModelSelector popup host', () => {
  test('selects models through Menu onClickMenuItem so portaled dropdowns still commit', () => {
    expect(source.includes('onClickMenuItem=')).toBe(true);
    expect(source.includes('findChatModelForMenuKey')).toBe(true);
    expect(source.includes('React.forwardRef')).toBe(true);
    expect(source.includes('...rest')).toBe(true);
    expect(source.includes('onClick={() => {\n                                setCurrentModel')).toBe(false);
    expect(source.includes('onClick={() => setSelectedAcpModel')).toBe(false);
  });
});
