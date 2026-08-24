import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('./TerminalRow.tsx', import.meta.url), 'utf8');

describe('TerminalRow title marquee contract', () => {
  test('uses the shared hover-only title reveal without changing row geometry', () => {
    expect(source.includes("import MarqueeText from '@/renderer/components/base/MarqueeText';")).toBe(true);
    expect(source.includes('<MarqueeText')).toBe(true);
    expect(source.includes("trigger='hover'")).toBe(true);
    expect(source.includes("title=''")).toBe(true);
    expect(source.includes('disabled={selectionMode || menuVisible}')).toBe(true);
    expect(source.includes('chat-history__item h-34px')).toBe(true);
  });
});
