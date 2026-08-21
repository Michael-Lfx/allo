

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

describe('WorkpathDrawer structure', () => {
  test('keeps secondary workpath actions behind a compact more menu', () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'WorkpathDrawer.tsx'), 'utf8');
    const hoverOpsIndex = source.indexOf('{/* Hover ops:');
    const moreButtonIndex = source.indexOf("data-testid='workpath-more-actions-btn'");

    expect(hoverOpsIndex).toBeGreaterThan(-1);
    expect(moreButtonIndex).toBeGreaterThan(hoverOpsIndex);
    expect(source.includes('<Dropdown')).toBe(true);
    expect(source.includes("trigger='click'")).toBe(true);
    expect(source.includes('getWorkpathMenuActionKeys')).toBe(true);
    expect(source.includes("<Menu.Item key='copy'>")).toBe(true);
    expect(source.includes("<Menu.Item key='pin'>")).toBe(true);
    expect(source.includes("<Menu.Item key='remove'>")).toBe(true);
    expect(source.includes("data-testid='workpath-create-interactive-btn'")).toBe(true);
    expect(source.includes('<CopyIconButton')).toBe(false);
    expect(source.includes('always visible (real workpaths only)')).toBe(false);
  });

  test('reveals workpath labels and paths without moving the action slot', () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'WorkpathDrawer.tsx'), 'utf8');

    expect(source.includes("import MarqueeText from '@/renderer/components/base/MarqueeText';")).toBe(true);
    expect(source.includes('<MarqueeText')).toBe(true);
    expect(source.includes("trigger='hover'")).toBe(true);
    expect(source.includes('useLayoutContext')).toBe(true);
    expect(source.includes('disabled={batchMode || isMobile}')).toBe(true);
    expect(source.includes('marqueeOnHover={!batchMode && !isMobile}')).toBe(true);
    expect(source.includes('marqueeActive={workpathIdentityHovered && !isMobile}')).toBe(true);
    expect(source.includes('onPointerEnter={() => setWorkpathIdentityHovered(true)}')).toBe(true);
    expect(source.includes('onPointerLeave={() => setWorkpathIdentityHovered(false)}')).toBe(true);
    expect(source.includes('absolute right-8px top-1/2')).toBe(true);
    expect(source.includes('group-focus-within:opacity-100')).toBe(true);
    expect(source.includes('group-focus-within:pointer-events-auto')).toBe(true);
    expect(source.includes('hidden group-hover:flex shrink-0 items-center gap-4px')).toBe(false);
    expect(source.includes("data-testid='workpath-more-actions-btn'")).toBe(true);
  });

  test('keeps a pinned workpath status visible outside the hover action slot', () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'WorkpathDrawer.tsx'), 'utf8');
    const indicatorIndex = source.indexOf("data-testid='workpath-pinned-badge'");
    const headerIconIndex = source.indexOf("className='relative size-22px flex items-center justify-center shrink-0 text-t-primary'");
    const identityIndex = source.indexOf("className='flex-1 min-w-0 flex items-center gap-6px overflow-hidden'");
    const hoverOpsIndex = source.indexOf('{/* Hover ops:');

    expect(indicatorIndex).toBeGreaterThan(-1);
    expect(headerIconIndex).toBeGreaterThan(-1);
    expect(identityIndex).toBeGreaterThan(headerIconIndex);
    expect(indicatorIndex).toBeGreaterThan(headerIconIndex);
    expect(indicatorIndex).toBeLessThan(identityIndex);
    expect(indicatorIndex).toBeLessThan(hoverOpsIndex);
    expect(source.includes("t('sessionList.pinnedWorkpath')")).toBe(true);
    expect(source.includes('bg-[rgb(var(--primary-6))]')).toBe(false);
    // Clears the 16px folder glyph: the marker hangs off the icon's top-right corner.
    expect(source.includes('absolute -top-4px -right-6px')).toBe(true);
    // Bare theme-adaptive accent glyph: no disc, no ring, no fixed white.
    expect(source.includes("theme='filled'")).toBe(true);
    expect(source.includes('text-[rgb(var(--primary-6))]')).toBe(true);
    expect(source.includes('size-12px rd-full')).toBe(false);
    expect(source.includes('border-2px border-solid border-[var(--color-bg-1)]')).toBe(false);
    expect(source.includes('text-white')).toBe(false);
    expect(source.includes('size-10px rd-3px')).toBe(false);
    expect(source.includes("title={t('sessionList.pinnedWorkpath')}")).toBe(false);
    expect(source.includes('workpath-pinned-indicator')).toBe(false);
    expect(source.includes('text-aou-1')).toBe(false);
    expect(source.includes('group-hover:hidden')).toBe(false);
    expect(source.includes("className='size-6px rd-full shrink-0 bg-aou-1")).toBe(false);
  });

  test('creates an interactive session directly from the workpath plus button', () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'WorkpathDrawer.tsx'), 'utf8');

    expect(source.includes("data-testid='workpath-create-interactive-btn'")).toBe(true);
    expect(source.includes("aria-label={t('sessionList.newInteractive')}")).toBe(true);
    expect(source.includes('onCreateInteractive(node);')).toBe(true);
    expect(source.includes('createMenuVisible')).toBe(false);
  });

  test('shows workspace details in a hover card', () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'WorkpathDrawer.tsx'), 'utf8');
    const cardSource = readFileSync(
      join(dirname(fileURLToPath(import.meta.url)), '../components/WorkpathHoverCard.tsx'),
      'utf8'
    );

    expect(source.includes("trigger='hover'")).toBe(true);
    expect(source.includes('<WorkpathHoverCard')).toBe(true);
    expect(source.includes('conversationCount={sessionCount}')).toBe(true);
    expect(source.includes('getPopupContainer={() => document.body}')).toBe(true);
    expect(source.includes('popupStyle: { padding: 0 }')).toBe(true);
    expect(source.includes("pointerEvents: 'none'")).toBe(true);
    // The row owns the hover actions, so the card's trigger starts inside the
    // row and closes before the action slot: hovering + / more never opens
    // the card, and the click-through popup can never swallow their events.
    expect(source.indexOf('<Popover')).toBeGreaterThan(source.indexOf('group group-hover:pr-50px'));
    expect(source.indexOf('</Popover>')).toBeLessThan(source.indexOf('{/* Hover ops:'));
    expect(cardSource.includes("t('sessionList.workpathConversationCount'")).toBe(true);
    expect(cardSource.includes('{workspacePath}')).toBe(true);
  });

  test('uses a light hover fill and a 2px workpath row rhythm', () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'WorkpathDrawer.tsx'), 'utf8');
    const conversationSource = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'ConversationRow.tsx'), 'utf8');

    expect(source.includes('hover:bg-fill-2')).toBe(true);
    expect(source.includes('hasInteractiveContent && \'gap-2px pt-2px\'')).toBe(true);
    expect(conversationSource.includes("'hover:bg-fill-2': !batchMode && !selected")).toBe(true);
  });

  test('renders interactive conversations directly without session-kind subgroups', () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'WorkpathDrawer.tsx'), 'utf8');
    const sessionListSource = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'index.tsx'), 'utf8');

    expect(source.includes("data-testid='workpath-conversation-list'")).toBe(true);
    expect(source.includes('visibleEntries.interactive.map((entry) => renderEntry(entry))')).toBe(true);
    expect(source.includes('SessionKindGroup')).toBe(false);
    expect(source.includes('visibleEntries.terminal')).toBe(false);
    expect(sessionListSource.includes('buildWorkpathTree(conversations, [], ui.pinnedKeys, emptyProjectWorkpaths)')).toBe(true);
    expect(sessionListSource.includes('onCreateTerminal={handleCreateTerminal}')).toBe(false);
  });
});
