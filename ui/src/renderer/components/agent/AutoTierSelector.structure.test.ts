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
    expect(source.includes("sendbox-responsive-reasoning-btn flowy-icon-text-btn")).toBe(true);
    expect(source.includes('popupVisible={popupVisible}')).toBe(true);
    expect(source.includes('onVisibleChange={handlePopupVisibleChange}')).toBe(true);
    expect(source.includes('popupVisible?: boolean')).toBe(true);
    expect(source.includes('useId')).toBe(true);
    expect(source.includes('sendbox-responsive-control-open')).toBe(true);
    expect(source.includes('aria-expanded={popupVisible}')).toBe(true);
    expect(source.includes("data-layout-part='leading-icon'")).toBe(true);
    expect(source.includes("data-layout-part='chevron'")).toBe(true);
    expect(source.includes("size='11'")).toBe(true);
    expect(source.includes('autoTextOnly')).toBe(true);
    expect(source.includes('AUTO_TIER_LABEL_FALLBACK')).toBe(true);
    expect(source.includes('auto-tier-trigger-label-slot')).toBe(true);
    expect(source.includes("className='sendbox-responsive-chevron shrink-0'")).toBe(true);
    expect(source.includes('useChatModelTriggerExpansion')).toBe(true);
    expect(source.includes("cssVariablePrefix: 'strategy'")).toBe(true);
    expect(source.includes("slotSelector: '.sendbox-strategy-slot'")).toBe(true);
    expect(source.includes('style={strategyTriggerExpansion.style}')).toBe(true);
    expect(source.includes('data-chat-strategy-expand-side')).toBe(true);
    expect(source.includes('Smart')).toBe(false);
    expect(source.includes('Intelligence')).toBe(false);
    expect(source.includes('Balance')).toBe(false);
    expect(source.includes('Cost')).toBe(false);
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

  test('guards the Nomi and Guid image-bearing send paths', () => {
    const nomiSource = readFileSync(
      new URL('../../pages/conversation/platforms/nomi/NomiSendBox.tsx', import.meta.url),
      'utf8',
    );
    const guidSource = readFileSync(new URL('../../pages/guid/GuidPage.tsx', import.meta.url), 'utf8');
    const guidSendSource = readFileSync(new URL('../../pages/guid/hooks/useGuidSend.ts', import.meta.url), 'utf8');

    expect(guidSource.includes('isNomiAgent && selectedChatModelOption?.family === \'auto\'')).toBe(true);
    expect(guidSendSource.includes('autoModelHasImageAttachments')).toBe(true);
    expect(nomiSource.includes('const canSendModelFiles')).toBe(true);
    expect(nomiSource.includes('if (!canSendModelFiles(filesToSend))')).toBe(true);
    expect(nomiSource.includes('if (!canSendModelFiles(files)')).toBe(true);
  });
});
