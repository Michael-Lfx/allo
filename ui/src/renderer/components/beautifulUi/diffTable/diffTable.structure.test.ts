import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./DiffTable.tsx', import.meta.url), 'utf8');
const cssSource = readFileSync(new URL('./diffTable.module.css', import.meta.url), 'utf8');

describe('DiffTable', () => {
  test('covers Beautiful UI file-change evidence without inventing message types', () => {
    expect(source.includes('export type DiffTableFile = {')).toBe(true);
    expect(source.includes('id: string')).toBe(true);
    expect(source.includes('title: string')).toBe(true);
    expect(source.includes('insertions: number')).toBe(true);
    expect(source.includes('deletions: number')).toBe(true);
    expect(source.includes("data-testid='beautiful-ui-diff-table'")).toBe(true);
    expect(source.includes('+{file.insertions}')).toBe(true);
    expect(source.includes('-{file.deletions}')).toBe(true);
    expect(source.includes('onFileClick?.(file)')).toBe(true);
    expect(source.includes('onDiffClick?.(file)')).toBe(true);
    expect(source.includes('TMessageType')).toBe(false);
    expect(cssSource.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
  });

  test('uses Beautiful UI Lucide glyphs instead of IconPark', () => {
    expect(source.includes("from 'lucide-react'")).toBe(true);
    expect(source.includes('ChevronDown')).toBe(true);
    expect(source.includes('@icon-park/react')).toBe(false);
  });
});
