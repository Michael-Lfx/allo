import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('ChatTitleEditor workspace subtitle contract', () => {
  test('keeps the optional read-only subtitle outside the rename branch', () => {
    const source = readSource(new URL('./ChatTitleEditor.tsx', import.meta.url));

    expect(source.includes('subtitle?: React.ReactNode')).toBe(true);
    expect(source.includes('subtitle,')).toBe(true);
    expect(source.includes('<MarqueeText')).toBe(true);
    expect(source.includes("trigger='hoverOrFocus'")).toBe(true);
    expect(source.includes("typeof title === 'string'")).toBe(true);
    expect(source.includes('{subtitle && (')).toBe(true);
    expect(source.indexOf('{subtitle && (')).toBeGreaterThan(source.indexOf('{editingTitle && canRenameTitle ?'));
  });

  test('keeps conversation search out of the title and in the header actions', () => {
    const titleSource = readSource(new URL('./ChatTitleEditor.tsx', import.meta.url));
    const layoutSource = readSource(new URL('./ChatLayout/index.tsx', import.meta.url));
    const actionsAt = layoutSource.indexOf('data-chat-header-actions');
    const searchAt = layoutSource.indexOf('<ConversationTitleMinimap');

    expect(titleSource.includes('ConversationTitleMinimap')).toBe(false);
    expect(titleSource.includes('hover:bg-3')).toBe(false);
    const surfaceSource = readSource(new URL('./ChatTitleEditor.module.css', import.meta.url));
    expect(titleSource.includes('group-hover:text-[rgb(var(--primary-6))]')).toBe(false);
    expect(surfaceSource.includes('color: var(--text-primary)')).toBe(true);
    expect(actionsAt).toBeGreaterThan(-1);
    expect(searchAt).toBeGreaterThan(actionsAt);
  });
});
