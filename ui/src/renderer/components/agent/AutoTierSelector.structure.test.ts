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
    expect(source.includes('popupVisible={effectivePopupVisible}')).toBe(true);
    expect(source.includes('onVisibleChange={handlePopupVisibleChange}')).toBe(true);
    expect(source.includes('popupVisible?: boolean')).toBe(true);
    expect(source.includes('useId')).toBe(true);
    expect(source.includes('sendbox-responsive-control-open')).toBe(true);
    expect(source.includes('aria-expanded={effectivePopupVisible}')).toBe(true);
    expect(source.includes("data-layout-part='leading-icon'")).toBe(true);
    expect(source.includes("data-layout-part='chevron'")).toBe(true);
    expect(source.includes("size='11'")).toBe(true);
    expect(source.includes('AUTO_TIER_LABEL_FALLBACK')).toBe(true);
    expect(source.includes('auto-tier-trigger-label-slot')).toBe(true);
    // The popup carries its own class so sendbox.css can align the Arco menu
    // rows (14px by default) with the composer menus' 13px row tier.
    expect(source.includes('auto-tier-selector-popup')).toBe(true);
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

  test('relies on the backend image self-healing chain instead of frontend send gates', () => {
    const nomiSource = readFileSync(
      new URL('../../pages/conversation/platforms/nomi/NomiSendBox.tsx', import.meta.url),
      'utf8',
    );
    const guidSource = readFileSync(new URL('../../pages/guid/GuidPage.tsx', import.meta.url), 'utf8');
    const guidSendSource = readFileSync(new URL('../../pages/guid/hooks/useGuidSend.ts', import.meta.url), 'utf8');

    // The per-message image count limit stays the only frontend attachment gate.
    expect(nomiSource.includes('const canSendModelFiles')).toBe(true);
    expect(nomiSource.includes('if (!canSendModelFiles(filesToSend))')).toBe(true);
    expect(nomiSource.includes('if (!canSendModelFiles(files, execution === undefined))')).toBe(true);

    // Image-bearing sends must not be blocked over the selected model: the
    // backend image_analyze fallback covers text-only and Auto-family models.
    expect(nomiSource.includes('autoModelHasImageAttachments')).toBe(false);
    expect(nomiSource.includes('autoTextOnly')).toBe(false);
    expect(nomiSource.includes('nomi-auto-image-warning')).toBe(false);
    expect(guidSource.includes('autoModelHasImageAttachments')).toBe(false);
    expect(guidSource.includes('autoTextOnly')).toBe(false);
    expect(guidSource.includes('guid-auto-image-warning')).toBe(false);
    expect(guidSendSource.includes('autoModelHasImageAttachments')).toBe(false);
    expect(guidSendSource.includes('autoTextOnly')).toBe(false);
  });
});
