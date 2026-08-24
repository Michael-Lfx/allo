

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

describe('CompanionSessionGroup structure', () => {
  test('uses sidebar overflow controls for long companion rosters', () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'CompanionSessionGroup.tsx'), 'utf8');

    expect(source.includes('getVisibleCompanionEntries')).toBe(true);
    expect(source.includes('showAllCompanions')).toBe(true);
    expect(source.includes('<SessionOverflowButton')).toBe(true);
    expect(source.includes("controlsId='flowy-companion-sessions'")).toBe(true);
    expect(source.includes('aria-expanded={expanded}')).toBe(true);
    expect(source.includes('overflowMotion.shouldRender')).toBe(true);
    expect(source.includes("aria-hidden={overflowMotion.phase === 'exiting'}")).toBe(true);
  });

  test('aligns the companion group with the workspace list', () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'CompanionSessionGroup.tsx'), 'utf8');
    const workpathDrawer = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'WorkpathDrawer.tsx'), 'utf8');
    const sessionList = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'index.tsx'), 'utf8');

    expect(sessionList).toContain('flowy-embedded-workpath-toolbar');
    expect(sessionList).toContain("'pl-10px pr-4px pb-6px'");
    expect(source).toContain("className='pl-10px pr-4px pb-6px flex items-center justify-between gap-8px min-w-0'");
    expect(source).toContain("className='sider-section-title appearance-none border-none bg-transparent p-0 text-13px font-[500] leading-none tracking-wide truncate shrink-0 opacity-75 transition-opacity hover:opacity-100 cursor-pointer'");
    expect(workpathDrawer).toContain('pl-10px pr-56px');
    expect(source).toContain('pl-10px pr-8px h-34px');
    expect(source).toContain("className='relative size-22px shrink-0 flex items-center justify-center'");
  });

  test('shows the purpose tip in a popup below the title icon', () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'CompanionSessionGroup.tsx'), 'utf8');

    expect(source.includes("import { Attention, Robot } from '@icon-park/react';")).toBe(true);
    expect(source.includes("t('sessionList.companionTip')")).toBe(true);
    expect(source.includes("<Attention theme='outline' size={12}")).toBe(true);
    expect(source.includes("position='bottom'")).toBe(true);
    expect(source.includes('bg-[rgba(var(--primary-6),0.06)]')).toBe(false);
  });

  test('nests each robot thread under its bound companion, not a separate bucket', () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'CompanionSessionGroup.tsx'), 'utf8');
    // Robot threads come from the shared list-sync snapshot (no extra fetch) and
    // are grouped by the companion they are bound to.
    expect(source.includes('useConversationListSync')).toBe(true);
    expect(source.includes('robotConversations')).toBe(true);
    expect(source.includes('robotsByCompanion')).toBe(true);
    expect(source.includes('companion_id')).toBe(true);
    // Device names share the one `/api/robots` request the robot tab uses.
    expect(source.includes("useSWR('robots.list'")).toBe(true);
    // Clicking a nested row opens that robot's own conversation.
    expect(source.includes('openRobotConversation')).toBe(true);
    // The standalone top-level bucket is gone.
    const listSource = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'index.tsx'), 'utf8');
    expect(listSource.includes('RobotSessionGroup')).toBe(false);
  });
});
