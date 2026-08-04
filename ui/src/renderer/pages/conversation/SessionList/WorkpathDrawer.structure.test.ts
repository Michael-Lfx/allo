

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

describe('WorkpathDrawer structure', () => {
  test('keeps copy path in the hover action group instead of a standalone resting icon', () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'WorkpathDrawer.tsx'), 'utf8');
    const hoverOpsIndex = source.indexOf('{/* Hover ops:');
    const copyButtonIndex = source.indexOf('<CopyIconButton');

    expect(hoverOpsIndex).toBeGreaterThan(-1);
    expect(copyButtonIndex).toBeGreaterThan(hoverOpsIndex);
    expect(source.includes('always visible (real workpaths only)')).toBe(false);
  });

  test('creates an interactive session directly from the workpath plus button', () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'WorkpathDrawer.tsx'), 'utf8');

    expect(source.includes("data-testid='workpath-create-interactive-btn'")).toBe(true);
    expect(source.includes("aria-label={t('sessionList.newInteractive')}")).toBe(true);
    expect(source.includes('onCreateInteractive(node);')).toBe(true);
    expect(source.includes('<Dropdown')).toBe(false);
    expect(source.includes('createMenuVisible')).toBe(false);
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
