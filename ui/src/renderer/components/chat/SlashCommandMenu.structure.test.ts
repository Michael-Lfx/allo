import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Slash command menu compact launcher variant', () => {
  test('keeps the compact launcher close to the panel edge with unfilled source labels', () => {
    const source = readSource(new URL('./SlashCommandMenu.tsx', import.meta.url));

    expect(source.includes('compact?: boolean')).toBe(true);
    expect(source.includes('{!compact && (')).toBe(true);
    expect(source.includes("compact ? 'overflow-y-auto px-8px py-4px'")).toBe(true);
    expect(source.includes("minHeight: compact ? '28px' : '38px'")).toBe(true);
    expect(source.includes("compact ? 'text-11px text-t-tertiary truncate'")).toBe(true);
    expect(source.includes('text-12px leading-20px shrink-0')).toBe(true);
    expect(source.includes("? 'text-t-tertiary'")).toBe(true);
    expect(source.includes("compact ? 'rounded-20px border border-solid overflow-hidden'")).toBe(true);
    expect(source.includes("color-mix(in srgb, var(--color-fill-2) 76%, var(--color-bg-1))")).toBe(true);
  });
});
