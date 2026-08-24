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

  test('gives conversation titles balanced left spacing', () => {
    expect(source.includes("!collapsed && 'pl-18px'")).toBe(true);
    expect(source.includes('showNestedPinnedBadge')).toBe(false);
    expect(source.includes('showHoverPinnedIcon && \'group-hover:pl-22px\'')).toBe(true);
    expect(source.includes('const showHoverPinnedIcon = !batchMode && isPinned && !isMobile && !isGenerating;')).toBe(true);
    expect(source.includes('!dimIcon && !batchMode && isPinned')).toBe(false);
    expect(source.includes("data-testid='conversation-pinned-badge'")).toBe(false);
    expect(source.includes('bg-[rgb(var(--primary-6))]')).toBe(false);
    expect(source.includes('size-8px -translate-y-1/2 rd-3px')).toBe(false);
    expect(source.includes('absolute left-18px top-1/2')).toBe(true);
    expect(source.includes("title={t('conversation.history.pinned')}")).toBe(false);
  });

  test('keeps trailing meta width stable so hover does not reflow the title', () => {
    expect(source.includes('hover:pr-40px')).toBe(false);
    expect(source.includes("'group-hover:hidden': !menuVisible")).toBe(false);
    expect(source.includes("'group-hover:invisible': !menuVisible")).toBe(true);
    expect(source.includes('invisible: menuVisible')).toBe(true);
  });

  test('keeps the active conversation in the hovered visual state outside batch mode', () => {
    expect(source.includes("'session-list-active-row !text-t-primary': selected && !batchMode")).toBe(true);
    expect(source.includes("'!bg-primary-1 !text-primary-6': selected")).toBe(false);
  });

  test('reveals long titles without moving the row actions', () => {
    expect(source.includes("import MarqueeText from '@/renderer/components/base/MarqueeText';")).toBe(true);
    expect(source.includes('<MarqueeText')).toBe(true);
    expect(source.includes("trigger='hover'")).toBe(true);
    expect(source.includes("title=''")).toBe(true);
    expect(source.includes('disabled={collapsed || batchMode || isMobile || menuVisible}')).toBe(true);
    expect(source.includes("right-8px top-1/2 -translate-y-1/2")).toBe(true);
  });

  test('uses a localized fallback for empty conversation titles', () => {
    expect(source.includes("const displayName = conversation.name?.trim() || t('conversation.historySearch.untitled');")).toBe(
      true
    );
    expect(source.includes('text={displayName}')).toBe(true);
    expect(source.includes('session-age-meta')).toBe(true);
  });
});
