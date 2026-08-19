/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('ReasoningEffortSelector structure', () => {
  test('uses the shared discrete slider surface instead of a dropdown menu', () => {
    const source = readSource(new URL('./ReasoningEffortSelector.tsx', import.meta.url));
    const css = readSource(new URL('./ReasoningEffortSelector.module.css', import.meta.url));

    expect(source.includes("import { Message, Popover, Slider } from '@arco-design/web-react'")).toBe(true);
    expect(source.includes('<Dropdown')).toBe(false);
    expect(source.includes('<Menu')).toBe(false);
    expect(source.includes('onAfterChange={onAfterChange}')).toBe(true);
    expect(source.includes('aria-valuetext')).toBe(true);
    expect(source.includes('data-testid=\'reasoning-effort-compact-trigger\'')).toBe(true);
    expect(source.includes("className='reasoning-effort-popover'")).toBe(true);
    expect(source.includes("classNames(styles.triggerIcon, 'shrink-0')")).toBe(true);
    expect(source.includes("classNames(styles.triggerLabelSlot, 'sendbox-responsive-label')")).toBe(true);
    expect(source.includes('styles.triggerLabelReserve')).toBe(true);
    expect(source.includes('styles.panelFooter')).toBe(false);
    expect(source.includes('tooltipVisible={false}')).toBe(true);
    expect(source.includes('styles.panelHeader')).toBe(false);
    expect(source.includes('styles.panelScale')).toBe(false);
    expect(source.includes('showTicks={false}')).toBe(true);
    expect(source.includes('TRACK_SPECK_POSITIONS')).toBe(false);
    expect(source.includes('trackSpeck')).toBe(false);
    expect(source.includes('styles.tick')).toBe(false);
    expect(source.includes('styles.levelMarker')).toBe(true);
    expect(source.includes('styles.levelMarkers')).toBe(true);
    expect(source.includes('data-max-active')).toBe(true);
    expect(source.includes('maximumActive')).toBe(true);
    expect(source.includes('MAXIMUM_CHARGE_PARTICLES')).toBe(true);
    expect(source.includes('styles.chargeParticle')).toBe(true);
    expect(source.includes('compactStatusText')).toBe(false);
    expect(source.includes('labelFor')).toBe(false);
    expect(source.includes('reasoningEffort.level.')).toBe(false);
    expect(css.includes('background: linear-gradient')).toBe(true);
    expect(css.includes('transform: scaleX(var(--effort-progress))')).toBe(true);
    expect(css.includes('width: min(224px, calc(100vw - 20px))')).toBe(true);
    expect(css.includes('display: flex')).toBe(true);
    expect(css.includes('height: auto')).toBe(true);
    expect(css.includes('min-height: 0')).toBe(true);
    expect(css.includes('grid-template-rows: 28px 20px')).toBe(false);
    expect(css.includes('grid-template-columns: minmax(0, 1fr) 64px')).toBe(false);
    expect(css.includes('inset: 0 12px')).toBe(true);
    expect(css.includes('.levelMarker')).toBe(true);
    expect(css.includes('left: 12px')).toBe(true);
    expect(css.includes('right: 12px')).toBe(true);
    expect(css.includes('.arco-slider-road) {\n  background: transparent;')).toBe(true);
    expect(css.includes('.compactTrigger > :global(.i-icon):first-child')).toBe(true);
    expect(css.includes('.triggerIcon')).toBe(true);
    expect(css.includes('margin-right: -2px')).toBe(true);
    expect(css.includes('transform: translateY(2px)')).toBe(false);
    expect(css.includes('.compactTrigger:active')).toBe(true);
    expect(css.includes('.arco-popover-content')).toBe(true);
    expect(css.includes('padding: 0 !important')).toBe(true);
    expect(css.includes('.trackSpeck')).toBe(false);
    expect(css.includes('.panelEffortChevron')).toBe(false);
    expect(css.includes('.panelLightning')).toBe(false);
    expect(css.includes('.panelFooter')).toBe(false);
    expect(css.includes('.panelStatus')).toBe(false);
    expect(css.includes('.chargeParticle')).toBe(true);
    expect(css.includes('@keyframes maximumChargeParticle')).toBe(true);
    expect(css.includes('@keyframes maximumChargeThumb')).toBe(true);
    expect(css.includes('var(--charge-travel)')).toBe(true);
    expect(css.includes('var(--particle-delay)')).toBe(true);
    expect(css.includes('var(--particle-opacity)')).toBe(true);
    expect(css.includes('data-max-active')).toBe(true);
    expect(css.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
  });

  test('uses one compact trigger and one body-mounted popover on every layout', () => {
    const source = readFileSync(
      new URL('../../pages/conversation/platforms/nomi/NomiSendBox.tsx', import.meta.url),
      'utf8'
    );
    const guidSource = readFileSync(new URL('../../pages/guid/GuidPage.tsx', import.meta.url), 'utf8');

    expect(source.includes('isProcessing={running}')).toBe(true);
    expect(source.includes('modelKey=')).toBe(true);
    expect(guidSource.includes('<ReasoningEffortSelector')).toBe(true);
    expect(guidSource.includes('modelKey=')).toBe(true);

    const selectorSource = readSource(new URL('./ReasoningEffortSelector.tsx', import.meta.url));
    const selectorCss = readSource(new URL('./ReasoningEffortSelector.module.css', import.meta.url));

    expect(selectorSource.includes('getPopupContainer={() => document.body}')).toBe(true);
    expect(selectorSource.includes("aria-haspopup='dialog'")).toBe(true);
    expect(selectorSource.includes('aria-controls={popoverId}')).toBe(true);
    expect(selectorSource.includes('aria-busy={isSaving || undefined}')).toBe(true);
    expect(selectorSource.includes("testId='reasoning-effort-popover-slider'")).toBe(true);
    expect(selectorSource.includes("testId='reasoning-effort-slider'")).toBe(false);
    expect((selectorSource.match(/<ReasoningSliderSurface \{\.\.\.sliderProps\}/g) ?? []).length).toBe(1);
    expect((selectorSource.match(/<Down/g) ?? []).length).toBe(1);
    expect(selectorSource.includes('styles.panelEffortChevron')).toBe(false);
    expect(selectorSource.includes('styles.inlineControl')).toBe(false);
    expect(selectorSource.includes('styles.panelIcon')).toBe(false);
    expect(selectorCss.includes('.inlineControl')).toBe(false);
    expect(selectorCss.includes('@container sendbox-config')).toBe(false);
    expect(selectorCss.includes('@container guid-action-config')).toBe(false);
    expect(selectorCss.includes("data-mobile")).toBe(false);
  });

  test('keeps the trigger hint localized in both supported locales', () => {
    const zh = JSON.parse(
      readFileSync(new URL('../../services/i18n/locales/zh-CN/conversation.json', import.meta.url), 'utf8')
    ) as { reasoningEffort: { hint: string; nextTurn: string; updated: string } };
    const en = JSON.parse(
      readFileSync(new URL('../../services/i18n/locales/en-US/conversation.json', import.meta.url), 'utf8')
    ) as { reasoningEffort: { hint: string; nextTurn: string; updated: string } };

    expect(zh.reasoningEffort.hint).toBe('点击打开并拖动调整推理深度');
    expect(en.reasoningEffort.hint).toBe('Click to open and drag to adjust reasoning depth');
    expect(zh.reasoningEffort.nextTurn).toBe('下一轮生效');
    expect(en.reasoningEffort.nextTurn).toBe('Applies next turn');
    expect(zh.reasoningEffort.updated).toBe('推理深度已应用');
    expect(en.reasoningEffort.updated).toBe('Reasoning depth applied');
  });
});
