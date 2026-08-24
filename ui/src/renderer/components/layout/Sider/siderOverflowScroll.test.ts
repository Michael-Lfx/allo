/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');
const styles = readFileSync(new URL('./Sider.module.css', import.meta.url), 'utf8');

describe('application sider overflow handling', () => {
  test('keeps primary navigation fixed and scrolls only the workspaces list', () => {
    expect(source.includes("data-testid='sider-primary-nav'")).toBe(true);
    expect(source.includes('styles.primaryNav')).toBe(true);
    expect(source.includes('shrink-0 flex flex-col gap-2px')).toBe(true);
    expect(source.includes("data-testid='sider-workspaces-scroll-area'")).toBe(true);
    expect(source.includes('styles.scrollArea')).toBe(true);
    expect(source.includes('flex-1 min-h-0 overflow-y-auto overflow-x-hidden pt-0 pb-8px')).toBe(true);
    expect(source.includes('styles.workspaceSection')).toBe(true);
    const workspaceHeadingStart = source.indexOf("id='flowy-workspaces-heading'");
    const scrollAreaStart = source.indexOf("data-testid='sider-workspaces-scroll-area'");
    expect(workspaceHeadingStart).toBeLessThan(scrollAreaStart);
  });

  test('keeps the settings group pinned', () => {
    expect(source.includes("'shrink-0 mt-auto pt-8px flex flex-col gap-2px")).toBe(true);
  });

  test('keeps the workspace heading in document flow and workpath headings sticky', () => {
    expect(styles.includes('.workspaceSectionHeader')).toBe(true);
    expect(styles.includes('.scrollArea :global(.flowy-workpath-drawer-header)')).toBe(true);
    expect(styles.includes('--flowy-sider-sticky-surface: color-mix')).toBe(true);
    expect(styles.includes('--flowy-sider-divider: color-mix')).toBe(true);
    const workspaceHeaderStart = styles.indexOf('.workspaceSectionHeader');
    const workpathHeaderStart = styles.indexOf(
      '.scrollArea :global(.flowy-workpath-drawer-header)',
    );
    const workpathHoverStart = styles.indexOf(
      '.scrollArea :global(.flowy-workpath-drawer-header:hover)',
    );

    expect(workspaceHeaderStart).toBeGreaterThan(-1);
    expect(workpathHeaderStart).toBeGreaterThan(workspaceHeaderStart);
    expect(workpathHoverStart).toBeGreaterThan(workpathHeaderStart);
    const workspaceHeader = styles.slice(workspaceHeaderStart, workpathHeaderStart);
    const workpathHeader = styles.slice(workpathHeaderStart, workpathHoverStart);

    expect(workspaceHeader.includes('position: static;')).toBe(true);
    expect(workspaceHeader.includes('position: sticky;')).toBe(false);
    expect(workpathHeader.includes('position: sticky;')).toBe(true);
    expect(workpathHeader.includes('top: 0;')).toBe(true);
    expect(
      workspaceHeader.includes('background: transparent;'),
    ).toBe(true);
    expect(workpathHeader.includes('background: var(--flowy-sider-sticky-surface);')).toBe(true);
    expect(styles.includes('border-bottom: 1px solid var(--flowy-sider-divider);')).toBe(true);
    const scrollPaddingStart = styles.indexOf('scroll-padding-block-start');
    const scrollbarWidthStart = styles.indexOf('scrollbar-width', scrollPaddingStart);
    const scrollPadding = styles.slice(scrollPaddingStart, scrollbarWidthStart);
    expect(scrollPadding.includes('var(--flowy-workpath-header-height)')).toBe(true);
    expect(scrollPadding.includes('var(--flowy-workspace-section-height)')).toBe(false);
  });
});
