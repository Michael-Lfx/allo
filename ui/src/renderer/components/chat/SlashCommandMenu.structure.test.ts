import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Slash command menu compact launcher variant', () => {
  test('hides the redundant header and reduces launcher row density', () => {
    const source = readSource(new URL('./SlashCommandMenu.tsx', import.meta.url));

    expect(source.includes('compact?: boolean')).toBe(true);
    expect(source.includes('{!compact && (')).toBe(true);
    expect(source.includes("compact ? 'overflow-y-auto px-2px py-2px'")).toBe(true);
    expect(source.includes("minHeight: compact ? '34px' : '38px'")).toBe(true);
    expect(source.includes("compact ? 'text-11px text-t-tertiary truncate'")).toBe(true);
  });
});
