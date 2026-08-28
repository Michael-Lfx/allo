/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');

describe('ChatLayout conversation plan tab', () => {
  test('merges a conversation-plan extra tab ahead of terminals and metrics', () => {
    expect(source.includes('CONVERSATION_PLAN_WORKSPACE_TAB')).toBe(true);
    expect(source.includes('ConversationPlanProvider')).toBe(true);
    expect(source.includes('ConversationPlanPanel')).toBe(true);
    expect(source.includes('mergedExtraTabs')).toBe(true);
    expect(source.includes('<ListCheckbox size={18} />')).toBe(true);
    expect(source.includes("key: CONVERSATION_PLAN_WORKSPACE_TAB")).toBe(true);
    expect(source.includes('plan.active && plan.done < plan.total')).toBe(true);
    expect(source.includes("className='workspace-tool-rail__badge'")).toBe(true);
    expect(source.includes('extraTabs={mergedExtraTabs}')).toBe(true);
    expect(source.includes('{ extraTabs: mergedExtraTabs }')).toBe(true);
    expect(source.includes('hidePlanChromeHeader')).toBe(true);
  });

  test('does not auto-open the plan tab when a plan arrives', () => {
    expect(source.includes('dispatchWorkspaceOpenPreviewTool')).toBe(false);
    expect(source.includes('if (!plan || !workspaceEnabled) return base')).toBe(true);
  });

  test('hides the duplicate workspace title chrome on the plan tab', () => {
    expect(source.includes('hidePlanChromeHeader')).toBe(true);
    expect(source.includes('activeWorkspaceTab === CONVERSATION_PLAN_WORKSPACE_TAB')).toBe(true);
  });
});
