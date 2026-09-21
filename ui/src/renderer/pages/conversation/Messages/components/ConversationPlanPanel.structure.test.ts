/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const panel = readFileSync(new URL('./ConversationPlanPanel.tsx', import.meta.url), 'utf8');
const list = readFileSync(new URL('./PlanTodoList.tsx', import.meta.url), 'utf8');
const css = readFileSync(new URL('./planTodoList.module.css', import.meta.url), 'utf8');
const context = readFileSync(new URL('./conversationPlanContext.tsx', import.meta.url), 'utf8');

describe('ConversationPlanPanel workspace tab', () => {
  test('renders a header strip with count and status, not a duplicate title', () => {
    expect(panel.includes("data-testid='conversation-plan-panel'")).toBe(true);
    expect(panel.includes("data-testid='conversation-plan-header'")).toBe(true);
    expect(panel.includes("data-testid='conversation-plan-spine'")).toBe(false);
    expect(panel.includes('spineFill')).toBe(false);
    expect(panel.includes('PlanThinkingOrb')).toBe(false);
    expect(panel.includes("listTestId='conversation-plan-list'")).toBe(true);
    expect(panel.includes("t('conversation.workspace.plan.tab'")).toBe(false);
    expect(panel.includes("t('conversation.workspace.plan.inProgress'")).toBe(true);
    expect(panel.includes("t('conversation.workspace.plan.incomplete'")).toBe(true);
    expect(panel.includes("t('conversation.workspace.plan.completed'")).toBe(true);
    expect(panel.includes('planDisplayStatus')).toBe(true);
    expect(panel.includes('displayPlanEntries')).toBe(true);
    expect(panel.includes("t('conversation.workspace.plan.empty'")).toBe(true);
    expect(panel.includes('<Empty')).toBe(true);
    expect(panel.includes("variant='panel'")).toBe(true);
    expect(panel.includes('conversation-plan-gooey')).toBe(false);
    expect(panel.includes('headerMetal')).toBe(false);
    expect(panel.includes('shimmerStyles')).toBe(false);
  });

  test('scrolls the in-progress row into view and honors reduced motion', () => {
    expect(panel.includes('inProgressRowRef')).toBe(true);
    expect(panel.includes("matchMedia('(prefers-reduced-motion: reduce)')")).toBe(true);
    expect(panel.includes("behavior: reduce ? 'auto' : 'smooth'")).toBe(true);
    expect(list.includes('inProgressRowRef')).toBe(true);
  });

  test('keeps compact and panel row chrome in the shared list', () => {
    expect(list.includes("case 'in_progress':")).toBe(true);
    expect(list.includes("case 'completed':")).toBe(true);
    expect(list.includes("case 'pending':")).toBe(true);
    expect(list.includes('const _exhaustive: never = status')).toBe(true);
    expect(list.includes('PlanThinkingOrb')).toBe(false);
    expect(list.includes("data-testid='conversation-plan-check'")).toBe(true);
    expect(list.includes("@icon-park/react")).toBe(false);
    expect(css.includes('-webkit-line-clamp: 2')).toBe(true);
    expect(css.includes('-webkit-line-clamp: 3')).toBe(true);
    expect(css.includes('plan-node-pulse')).toBe(false);
    expect(css.includes('spineFill')).toBe(false);
    expect(css.includes('checkCompleted')).toBe(true);
    expect(css.includes('checkWorking')).toBe(true);
    expect(css.includes('--plan-check: 18px')).toBe(true);
    expect(css.includes('grid-template-columns: var(--plan-check) minmax(0, 1fr)')).toBe(true);
    expect(css.includes('letter-spacing: -0.015em')).toBe(true);
    expect(css.includes('font-feature-settings: \'tnum\' 1')).toBe(true);
    expect(css.includes('prefers-reduced-motion: reduce')).toBe(true);
    expect(css.includes('backdrop-filter: saturate(180%) blur(20px)')).toBe(true);
  });

  test('exports a stable workspace tab key for the rail merge', () => {
    expect(context.includes("export const CONVERSATION_PLAN_WORKSPACE_TAB = 'conversation-plan'")).toBe(true);
    expect(context.includes('dispatchWorkspaceOpenPreviewTool')).toBe(true);
  });
});
