/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./PreviewTabs.tsx', import.meta.url), 'utf8');
const chromeCss = readFileSync(new URL('./preview.css', import.meta.url), 'utf8');

describe('preview chrome tab strip', () => {
  test('uses the list-checkbox glyph for the conversation plan workspace tab', () => {
    expect(source.includes("case 'conversation-plan':")).toBe(true);
    expect(source.includes('ListCheckbox')).toBe(true);
  });

  test('uses a compact bar with inset pill chips instead of full-height strips', () => {
    expect(chromeCss.includes('height: 32px;')).toBe(true);
    expect(source.includes('h-24px')).toBe(true);
    expect(source.includes('rd-6px')).toBe(true);
    expect(source.includes('text-12px')).toBe(true);
    expect(source.includes("minHeight: '36px'")).toBe(false);
    expect(source.includes('h-36px')).toBe(false);
  });

  test('exposes a plus menu for file, terminal, and browser tabs', () => {
    expect(source.includes("t('preview.addFile')")).toBe(true);
    expect(source.includes("t('preview.addTerminal')")).toBe(true);
    expect(source.includes("t('preview.addBrowser')")).toBe(true);
    expect(source.includes("t('preview.addTab')")).toBe(true);
  });
});

describe('preview add-file focuses the singleton files workspace tab', () => {
  test('does not open a file picker from the plus menu', () => {
    const panel = readFileSync(new URL('./PreviewPanel.tsx', import.meta.url), 'utf8');
    expect(panel.includes('DirectorySelectionModal')).toBe(false);
    expect(panel.includes('setFilePickerVisible')).toBe(false);
    expect(panel.includes("tab.key === 'files'")).toBe(true);
    expect(panel.includes('handleOpenWorkspaceTab(filesDefinition)')).toBe(true);
  });
});

describe('preview chrome tab strip continued', () => {
  test('hides the close control until hover, focus, or the active chip', () => {
    expect(source.includes('group-hover:opacity-100')).toBe(true);
    expect(source.includes('group-focus-within:opacity-100')).toBe(true);
  });

  test('offers accessible workspace-view navigation on mobile', () => {
    expect(source.includes("t('preview.workspaceViews')")).toBe(true);
    expect(source.includes('onOpenWorkspaceTab')).toBe(true);
    expect(source.includes('showWorkspaceMenu')).toBe(true);
  });

  test('places the plus control after the scrollable chips so overflow clips from the left', () => {
    const scroller = source.indexOf("className='preview-tabs__scroller'");
    const addMenu = source.indexOf("className='preview-tabs__add'");
    const closePanel = source.indexOf('{onClosePanel &&');
    expect(scroller).toBeGreaterThan(-1);
    expect(addMenu).toBeGreaterThan(scroller);
    expect(closePanel).toBeGreaterThan(addMenu);
    expect(source.includes('{addTabMenu}')).toBe(true);
    expect(chromeCss.includes('.preview-tabs__scroller-wrap')).toBe(true);
    expect(chromeCss.includes('flex: 0 1 auto;')).toBe(true);
    expect(chromeCss.includes('width: max-content;')).toBe(true);
  });

  test('makes active and hover chips readable against the tab bar', () => {
    expect(source.includes('font-medium border-[var(--color-border-2)]')).toBe(true);
    expect(source.includes('hover:bg-3 hover:text-t-primary')).toBe(true);
  });

  test('keeps the collapse control fully visible at the trailing edge', () => {
    expect(source.includes('preview-tabs__collapse-btn')).toBe(true);
    expect(source.includes('pr-10px')).toBe(false);
    expect(chromeCss.includes('padding: 0 8px 0 4px;')).toBe(true);
    expect(chromeCss.includes('.preview-tabs {')).toBe(true);
    expect(chromeCss.includes('overflow: hidden;')).toBe(true);
    expect(chromeCss.includes('.preview-tabs__collapse')).toBe(true);
    expect(chromeCss.includes('margin-left: auto;')).toBe(true);
  });

  test('hides the tab scroller chrome while keeping horizontal overflow', () => {
    expect(chromeCss.includes('.preview-tabs__scroller')).toBe(true);
    expect(chromeCss.includes('scrollbar-width: none;')).toBe(true);
    expect(chromeCss.includes('.preview-tabs__scroller::-webkit-scrollbar')).toBe(true);
    expect(chromeCss.includes('overflow-x: auto;')).toBe(true);
  });
});

describe('tab overflow wheel pan', () => {
  test('binds wheel after the scroller mounts so closed-panel first paint cannot miss the node', () => {
    const overflow = readFileSync(new URL('../../hooks/useTabOverflow.ts', import.meta.url), 'utf8');
    expect(overflow.includes('setScrollerNode(node)')).toBe(true);
    expect(overflow.includes("addEventListener('wheel', handleWheel, { passive: false })")).toBe(true);
    expect(overflow.includes('[scrollerNode, updateTabOverflow]')).toBe(true);
  });
});

describe('preview chrome breadcrumb', () => {
  test('renders workspace folder hierarchy on the toolbar, not only the leaf filename', () => {
    const toolbar = readFileSync(new URL('./PreviewToolbar.tsx', import.meta.url), 'utf8');
    expect(toolbar.includes("t('preview.pathLabel')")).toBe(true);
    expect(toolbar.includes('breadcrumbSegments')).toBe(true);
    expect(toolbar.includes("{'>'}")).toBe(true);
  });
});

describe('workspace tab host', () => {
  test('keeps workspace views in a flex column so file tree, terminal, and metrics can fill the tab', () => {
    const panel = readFileSync(new URL('./PreviewPanel.tsx', import.meta.url), 'utf8');
    expect(panel.includes("style={{ display: visible ? 'flex' : 'none' }}")).toBe(true);
    expect(panel.includes(' hidden={!visible}')).toBe(false);
    expect(panel.includes("className='min-h-0 flex-1 flex flex-col overflow-hidden'")).toBe(true);
  });

  test('resyncs the workspace sider when close restores a workspace preview tab', () => {
    const panel = readFileSync(new URL('./PreviewPanel.tsx', import.meta.url), 'utf8');
    expect(panel.includes('syncWorkspaceAfterClose')).toBe(true);
    expect(panel.includes('onWorkspaceTabActivate?.(nextTab.workspaceTabKey)')).toBe(true);
  });
});
