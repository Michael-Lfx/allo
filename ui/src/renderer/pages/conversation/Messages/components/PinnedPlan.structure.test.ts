/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./PinnedPlan.tsx', import.meta.url), 'utf8');
const nomiChatSource = readFileSync(new URL('../../platforms/nomi/NomiChat.tsx', import.meta.url), 'utf8');
const nomiSendBoxSource = readFileSync(new URL('../../platforms/nomi/NomiSendBox.tsx', import.meta.url), 'utf8');
const sendBoxSource = readFileSync(new URL('../../../../components/chat/SendBox/index.tsx', import.meta.url), 'utf8');
const planListSource = readFileSync(new URL('./PlanTodoList.tsx', import.meta.url), 'utf8');
const css = readFileSync(new URL('./planTodoList.module.css', import.meta.url), 'utf8');

describe('PinnedPlan composer chip', () => {
  test('uses a content-fit material capsule with in-flow checklist expansion', () => {
    expect(source.includes("data-testid='pinned-plan-bar'")).toBe(true);
    expect(source.includes("data-testid='pinned-plan-summary'")).toBe(true);
    expect(source.includes("data-testid='pinned-plan-progress'")).toBe(false);
    expect(source.includes("data-testid='pinned-plan-progress-indicator'")).toBe(true);
    expect(source.includes("listTestId='pinned-plan-list'")).toBe(true);
    expect(source.includes('sm:w-[56%]')).toBe(false);
    expect(source.includes('max-w-[520px]')).toBe(false);
    expect(source.includes('w-fit max-w-[calc(100vw-32px)]')).toBe(true);
    expect(source.includes('flex flex-col items-center')).toBe(true);
    expect(source.includes("from 'thinking-orbs'")).toBe(false);
    expect(source.includes("from '@icon-park/react'")).toBe(false);
    expect(source.includes('planDisplayStatus')).toBe(true);
    expect(source.includes("data-testid='pinned-plan-popover'")).toBe(true);
    expect(source.includes('absolute left-1/2 w-[min(320px,calc(100vw-32px))] -translate-x-1/2 bottom-full')).toBe(
      false
    );
    expect(source.includes('w-[min(320px,calc(100vw-32px))] mb-6px')).toBe(true);
    expect(css.includes('backdrop-filter: saturate(180%) blur(20px)')).toBe(true);
    expect(css.includes('.chip {')).toBe(true);
    expect(planListSource.includes('conversation-plan-check')).toBe(true);
  });

  test('opens the workspace plan tab on desktop instead of a hover popover', () => {
    expect(source.includes('onMouseEnter=')).toBe(false);
    expect(source.includes('onMouseLeave=')).toBe(false);
    expect(source.includes('const DESKTOP_CLOSE_DELAY_MS')).toBe(false);
    expect(source.includes('canOpenPlanTab')).toBe(true);
    expect(source.includes('openPlanTab()')).toBe(true);
    expect(source.includes('showInPlaceList')).toBe(true);
    expect(source.includes("variant='compact'")).toBe(true);
    expect(planListSource.includes("variant: PlanTodoListVariant")).toBe(true);
  });

  test('is centered above the sendbox panel in document flow', () => {
    expect(nomiChatSource.includes('<PinnedPlan />')).toBe(false);
    expect(nomiSendBoxSource.includes('showPinnedPlan')).toBe(true);
    expect(sendBoxSource.includes('showPinnedPlan?: boolean')).toBe(true);
    expect(sendBoxSource.includes('topRightTools?: React.ReactNode')).toBe(true);
    expect(sendBoxSource.includes("data-testid='sendbox-plan-anchor'")).toBe(true);
    expect(sendBoxSource.includes('<PinnedPlan plan={pinnedPlan} active={Boolean(loading || isLoading)} />')).toBe(true);
    expect(sendBoxSource.includes('useConversationPlan')).toBe(true);
    expect(sendBoxSource.includes('setConversationPlan(pinnedPlan)')).toBe(true);
    expect(sendBoxSource.includes("data-testid='sendbox-top-right-tools'")).toBe(false);
    expect(sendBoxSource.includes("data-testid='sendbox-internal-status-row'")).toBe(true);
    expect(sendBoxSource.includes("data-testid='sendbox-internal-plan'")).toBe(false);
    expect(sendBoxSource.includes("data-testid='sendbox-internal-context-tools'")).toBe(true);
    expect(sendBoxSource.includes('absolute left-1/2 bottom-[calc(100%+8px)] -translate-x-1/2')).toBe(false);
    expect(sendBoxSource.includes('relative z-1 mb-8px flex justify-center')).toBe(true);
    expect(sendBoxSource.includes('max-w-[420px]')).toBe(false);
    expect(sendBoxSource.includes('flex-[1_1_340px]')).toBe(false);
    expect(sendBoxSource.includes("data-testid='sendbox-top-row'")).toBe(false);
    expect(sendBoxSource.includes('top-1/2 -translate-y-1/2')).toBe(false);
    expect(nomiSendBoxSource.includes('topRightTools=')).toBe(false);

    const composerSurfaceIndex = sendBoxSource.indexOf('<ComposerSurface');
    const panelIndex = sendBoxSource.indexOf('panelClassName={`sendbox-panel');
    const beforeIndex = sendBoxSource.indexOf('before={');
    const anchorIndex = sendBoxSource.indexOf("data-testid='sendbox-plan-anchor'");
    const pinnedIndex = sendBoxSource.indexOf("data-testid='sendbox-internal-status-row'");
    expect(composerSurfaceIndex).toBeGreaterThan(-1);
    expect(anchorIndex).toBeGreaterThan(-1);
    expect(panelIndex).toBeGreaterThan(-1);
    expect(panelIndex).toBeGreaterThan(composerSurfaceIndex);
    expect(beforeIndex).toBeGreaterThan(panelIndex);
    expect(anchorIndex).toBeGreaterThan(beforeIndex);
    expect(pinnedIndex).toBeGreaterThan(panelIndex);
  });
});
