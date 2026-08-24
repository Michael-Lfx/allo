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
});
