import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('./PathText.tsx', import.meta.url), 'utf8');

describe('PathText marquee contract', () => {
  test('keeps splitPath as the resting representation and reveals the raw path', () => {
    expect(source.includes('marqueeOnHover?: boolean')).toBe(true);
    expect(source.includes('marqueeActive?: boolean')).toBe(true);
    expect(source.includes('splitPath(path)')).toBe(true);
    expect(source.includes('<MarqueeText')).toBe(true);
    expect(source.includes('text={path}')).toBe(true);
    expect(source.includes('staticContent={staticContent}')).toBe(true);
    expect(source.includes('active={marqueeActive}')).toBe(true);
  });

  test('keeps the existing static path structure when marquee is disabled', () => {
    expect(source.includes("if (!marqueeOnHover)")).toBe(true);
    expect(source.includes("'flex items-center min-w-0 overflow-hidden'")).toBe(true);
    expect(source.includes("'truncate'")).toBe(true);
  });
});
