import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./ContextCards.tsx', import.meta.url), 'utf8');
const modelSource = readFileSync(new URL('./contextCardModel.ts', import.meta.url), 'utf8');
const cssSource = readFileSync(new URL('./contextCards.module.css', import.meta.url), 'utf8');

describe('ContextCards', () => {
  test('covers Beautiful UI context cards without inventing message types', () => {
    expect(source.includes("data-testid='beautiful-ui-context-card'")).toBe(true);
    expect(
      modelSource.includes("export type ContextCardSourceKind = 'pdf' | 'csv' | 'md' | 'code' | 'other'")
    ).toBe(true);
    expect(source.includes("const exhaustive: never = item.sourceKind")).toBe(false);
    expect(source.includes('const exhaustive: never = kind')).toBe(true);
    expect(cssSource.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
  });

  test('uses Beautiful UI Lucide glyphs instead of IconPark', () => {
    expect(source.includes("from 'lucide-react'")).toBe(true);
    expect(source.includes('FileSpreadsheet')).toBe(true);
    expect(source.includes('FileCode2')).toBe(true);
    expect(source.includes('@icon-park/react')).toBe(false);
  });
});
