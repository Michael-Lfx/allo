/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

describe('AutoTierSelector structure', () => {
  test('keeps Auto tiers separate from Cloud reasoning effort', () => {
    const source = readFileSync(new URL('./AutoTierSelector.tsx', import.meta.url), 'utf8');

    expect(source.includes('AUTO_TIER_ORDER')).toBe(true);
    expect(source.includes("data-testid='auto-tier-selector'" )).toBe(true);
    expect(source.includes("data-testid='auto-tier-selector-popup'" )).toBe(true);
    expect(source.includes('autoTextOnly')).toBe(true);
    expect(source.includes('reasoning_effort')).toBe(false);
    expect(source.includes('"auto"')).toBe(false);
  });

  test('Nomi and Guid choose Auto tiers only for catalog Auto models', () => {
    const nomiSource = readFileSync(
      new URL('../../pages/conversation/platforms/nomi/NomiSendBox.tsx', import.meta.url),
      'utf8',
    );
    const guidSource = readFileSync(new URL('../../pages/guid/GuidPage.tsx', import.meta.url), 'utf8');

    expect(nomiSource.includes("selectedChatModelOption?.family === 'auto'" )).toBe(true);
    expect(guidSource.includes("selectedChatModelOption?.family === 'auto'" )).toBe(true);
    expect(nomiSource.includes('reasoning_effort: \'auto\'' )).toBe(false);
    expect(guidSource.includes('reasoning_effort: \'auto\'' )).toBe(false);
  });
});
