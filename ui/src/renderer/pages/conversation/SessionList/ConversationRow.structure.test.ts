/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./ConversationRow.tsx', import.meta.url), 'utf8');

describe('ConversationRow structure', () => {
  test('does not render a logo or reserve its leading slot', () => {
    expect(source.includes('getAgentLogo')).toBe(false);
    expect(source.includes('usePresetInfo')).toBe(false);
    expect(source.includes('renderLeadingIcon')).toBe(false);
    expect(source.includes("{isGenerating && !batchMode && <Spin size={16} />}")).toBe(true);
  });

  test('keeps trailing meta width stable so hover does not reflow the title', () => {
    expect(source.includes('hover:pr-40px')).toBe(false);
    expect(source.includes("'group-hover:hidden': !menuVisible")).toBe(false);
    expect(source.includes("'group-hover:invisible': !menuVisible")).toBe(true);
    expect(source.includes('invisible: menuVisible')).toBe(true);
  });
});
