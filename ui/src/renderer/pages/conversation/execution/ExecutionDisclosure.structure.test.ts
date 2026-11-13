import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./ExecutionDisclosure.tsx', import.meta.url), 'utf8');

describe('ExecutionDisclosure', () => {
  test('uses an accessible button disclosure with execution-scoped persistence', () => {
    expect(source.includes("type='button'")).toBe(true);
    expect(source.includes('aria-expanded={expanded}')).toBe(true);
    expect(source.includes('aria-controls={contentId}')).toBe(true);
    expect(source.includes("nomifun:execution-panel-sections:")).toBe(true);
    expect(source.includes('localStorage.setItem')).toBe(true);
  });
});
