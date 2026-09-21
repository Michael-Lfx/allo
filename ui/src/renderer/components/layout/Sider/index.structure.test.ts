import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');

describe('Primary sider structure', () => {
  test('keeps the navigation grouped by product intent', () => {
    expect(source.includes("t('common.titlebar.sections.work')")).toBe(true);
    expect(source.includes("t('common.titlebar.sections.resources')")).toBe(true);
    expect(source.includes("t('common.titlebar.sections.automation')")).toBe(true);
    expect(source.includes("t('common.titlebar.sections.workspaces')")).toBe(true);
    expect((source.match(/<SiderSectionHeader/g) ?? []).length).toBe(4);
  });

  test('gives the independently scrolling workpath list a semantic workspace region', () => {
    expect(source.includes("data-testid='sider-workspaces-section'")).toBe(true);
    expect(source.includes("aria-labelledby='flowy-workspaces-heading'")).toBe(true);
    expect(source.includes("id='flowy-workspaces-heading'")).toBe(true);
    expect(source.includes('embeddedInPrimarySider')).toBe(true);
    expect(source.includes("data-testid='sider-workspace-actions-target'")).toBe(true);
    expect(source.includes('workspaceActionsTarget={workspaceActionsTarget}')).toBe(true);
    const workspaceHeadingStart = source.indexOf("id='flowy-workspaces-heading'");
    const workspaceActionsStart = source.indexOf("data-testid='sider-workspace-actions-target'");
    const scrollAreaStart = source.indexOf("data-testid='sider-workspaces-scroll-area'");
    expect(workspaceHeadingStart).toBeLessThan(scrollAreaStart);
    expect(workspaceActionsStart).toBeLessThan(scrollAreaStart);
  });

  test('exposes a stable control target for the sidebar toggle', () => {
    expect(source.includes("id='flowy-primary-sider'")).toBe(true);
  });

  test('expanded icon dock still shows hover menu names', () => {
    const dockEntries = [
      './SiderNav/SiderKnowledgeEntry.tsx',
      './SiderNav/SiderLearningEntry.tsx',
      './SiderNav/SiderScheduledEntry.tsx',
      './SiderNav/SiderMeetingEntry.tsx',
    ];

    for (const relativePath of dockEntries) {
      const entrySource = readFileSync(new URL(relativePath, import.meta.url), 'utf8');
      const dockStart = entrySource.indexOf('if (dock)');
      expect(dockStart).toBeGreaterThan(-1);
      const instantIdx = entrySource.indexOf('<InstantHoverTooltip', dockStart);
      const tooltipIdx = entrySource.indexOf('<Tooltip', dockStart);
      expect(instantIdx).toBeGreaterThan(dockStart);
      expect(tooltipIdx === -1 || instantIdx < tooltipIdx).toBe(true);
    }
  });
});
