/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const stylesheet = readFileSync(new URL('./chat-layout.css', import.meta.url), 'utf8');
const componentSource = readFileSync(new URL('./WorkspaceToolRail.tsx', import.meta.url), 'utf8');
const chatLayoutSource = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');
const terminalSessionPageSource = readFileSync(
  new URL('../../../terminal/TerminalSessionPage.tsx', import.meta.url),
  'utf8',
);
const workspaceEventsSource = readFileSync(
  new URL('../../../../utils/workspace/workspaceEvents.ts', import.meta.url),
  'utf8',
);
const workspaceRailBodySource = readFileSync(new URL('../../Workspace/WorkspaceRailBody.tsx', import.meta.url), 'utf8');
const workspaceEventsHookSource = readFileSync(new URL('../../Workspace/hooks/useWorkspaceEvents.ts', import.meta.url), 'utf8');

const rule = (selector: string) => {
  const match = stylesheet.match(new RegExp(`${selector}\\s*\\{([\\s\\S]*?)\\n\\}`, 'm'));
  expect(match).not.toBeNull();
  return match?.[1] ?? '';
};

describe('workspace tool rail dimensions', () => {
  test('uses a text-free red dot when workspace changes are pending', () => {
    const badge = rule('\\.workspace-tool-rail__badge');

    expect(componentSource.includes("changeCount > 0 ? <span className='workspace-tool-rail__badge' /> : undefined")).toBe(true);
    expect(componentSource.includes("changeCount > 99 ? '99+' : changeCount")).toBe(false);
    expect(badge.includes('width: 7px;')).toBe(true);
    expect(badge.includes('height: 7px;')).toBe(true);
    expect(badge.includes('background: rgb(var(--danger-6));')).toBe(true);
  });

  test('refreshes the change count from the existing agent workspace refresh signal', () => {
    expect(workspaceRailBodySource.includes('refreshChanges: fileChangesHook.refreshChanges,')).toBe(true);
    expect(workspaceEventsHookSource.includes('refreshChangesRef.current();')).toBe(true);
  });

  test('uses compact square desktop controls', () => {
    const rail = rule('\\.workspace-tool-rail');
    const item = rule('\\.workspace-tool-rail__item');
    const collapse = rule('\\.workspace-tool-rail__item--collapse');

    expect(rail.includes('flex: 0 0 32px;')).toBe(true);
    expect(rail.includes('width: 32px;')).toBe(true);
    expect(rail.includes('min-width: 32px;')).toBe(true);
    expect(rail.includes('z-index: 12;')).toBe(false);
    expect(rail.includes('box-shadow:')).toBe(false);
    expect(item.includes('width: 28px;')).toBe(true);
    expect(item.includes('height: 28px;')).toBe(true);
    expect(item.includes('aspect-ratio: 1 / 1;')).toBe(true);
    expect(collapse.includes('height: 28px;')).toBe(true);
  });

  test('uses the same readable icon color for the collapse control as the toolbar controls', () => {
    const item = rule('\\.workspace-tool-rail__item');
    const collapse = rule('\\.workspace-tool-rail__item--collapse');

    expect(item.includes('color: var(--text-secondary);')).toBe(true);
    expect(collapse.includes('color: var(--text-secondary);')).toBe(true);
  });

  test('sets an explicit theme-aware color for the workspace panel title', () => {
    const title = rule('\\.workspace-panel-header__title');

    expect(title.includes('color: var(--text-primary);')).toBe(true);
  });

  test('does not change the mobile workspace trigger dimensions', () => {
    const trigger = rule('\\.workspace-tool-rail-mobile-trigger');

    expect(trigger.includes('width: 24px;')).toBe(true);
    expect(trigger.includes('height: 70px;')).toBe(true);
  });

  test('keeps labels accessible but visually hidden beneath icon-only controls', () => {
    const label = rule('\\.workspace-tool-rail__label');

    expect(componentSource.includes("className='workspace-tool-rail__label'")).toBe(true);
    expect(label.includes('position: absolute;')).toBe(true);
    expect(label.includes('width: 1px;')).toBe(true);
    expect(label.includes('height: 1px;')).toBe(true);
    expect(label.includes('overflow: hidden;')).toBe(true);
  });

  test('uses a compact scoped tooltip and removes the active vertical bar', () => {
    const tooltip = rule('\\.workspace-tool-rail__tooltip \\.arco-tooltip-content');

    expect(componentSource.includes("mini className='workspace-tool-rail__tooltip'")).toBe(true);
    expect(stylesheet.includes('.workspace-tool-rail__item--active::before')).toBe(false);
    expect(tooltip.includes('font-size: 11px;')).toBe(true);
    expect(tooltip.includes('line-height: 16px;')).toBe(true);
    expect(tooltip.includes('padding: 3px 6px;')).toBe(true);
  });
});

/**
 * Structural coverage only: bun cannot mount the full conversation/terminal
 * pages here, so these assertions verify both surfaces subscribe through the
 * typed entry point and keep the existing `files` selection behavior.
 */
describe('workspace titlebar toggle subscription wiring', () => {
  test('subscribes the conversation surface and keeps the files selection', () => {
    expect(chatLayoutSource).toContain(
      "import { subscribeWorkspaceToggleEvent } from '@/renderer/utils/workspace/workspaceEvents';",
    );
    expect(chatLayoutSource).toContain('subscribeWorkspaceToggleEvent(workspaceTarget, () => {');
    expect(chatLayoutSource).toContain("selectWorkspaceTool('files');");
    expect(chatLayoutSource).toContain(
      'if (typeof window === \'undefined\' || !workspaceTarget || !workspaceEnabled) return undefined;',
    );
    expect(chatLayoutSource).toContain(
      '}, [selectWorkspaceTool, workspaceEnabled, workspaceTarget]);',
    );
    expect(chatLayoutSource.includes('WORKSPACE_TOGGLE_EVENT')).toBe(false);
    expect(chatLayoutSource.includes('nomifun-workspace-toggle')).toBe(false);
  });

  test('subscribes the terminal surface and keeps the files selection', () => {
    expect(terminalSessionPageSource).toContain(
      "import { subscribeWorkspaceToggleEvent } from '@/renderer/utils/workspace/workspaceEvents';",
    );
    expect(terminalSessionPageSource).toContain('subscribeWorkspaceToggleEvent(workspaceTarget, () => {');
    expect(terminalSessionPageSource).toContain("selectWorkspaceTool('files');");
    expect(terminalSessionPageSource).toContain("if (typeof window === 'undefined') return undefined;");
    expect(terminalSessionPageSource).toContain('}, [selectWorkspaceTool, workspaceTarget]);');
    expect(terminalSessionPageSource.includes('WORKSPACE_TOGGLE_EVENT')).toBe(false);
    expect(terminalSessionPageSource.includes('nomifun-workspace-toggle')).toBe(false);
  });

  test('keeps the has-files event unchanged on its own custom event path', () => {
    expect(workspaceEventsSource).toContain(
      "export const WORKSPACE_HAS_FILES_EVENT = 'nomifun-workspace-has-files';",
    );
    expect(workspaceEventsSource).toContain('window.dispatchEvent(');
    expect(workspaceEventsSource.includes('WORKSPACE_TOGGLE_EVENT')).toBe(false);
    expect(workspaceEventsSource).toContain("emitter.emit('workspace.toggle', { target });");
  });
});
