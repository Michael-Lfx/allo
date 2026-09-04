import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import { hunksFromOldNew, hunksFromUnifiedDiff, countDiffStats, countDiffLines } from './inlineDiffModel';

const source = readFileSync(new URL('./InlineDiff.tsx', import.meta.url), 'utf8');
const cssSource = readFileSync(new URL('./inlineDiff.module.css', import.meta.url), 'utf8');

describe('InlineDiff', () => {
  test('covers Beautiful UI collapsible hunk chrome without inventing message types', () => {
    expect(source.includes('export type InlineDiffProps = {')).toBe(true);
    expect(source.includes('filename: string')).toBe(true);
    expect(source.includes('hunks: InlineDiffHunk[]')).toBe(true);
    expect(source.includes("data-testid='beautiful-ui-inline-diff'")).toBe(true);
    expect(source.includes('aria-expanded={expanded}')).toBe(true);
    expect(source.includes('aria-controls={bodyId}')).toBe(true);
    expect(source.includes("t('common.viewMoreLines'")).toBe(true);
    expect(source.includes("t('common.collapse')")).toBe(true);
    expect(source.includes('TMessageType')).toBe(false);
    expect(cssSource.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
    expect(cssSource.includes('white-space: pre-wrap')).toBe(true);
  });

  test('uses Beautiful UI Lucide glyphs instead of IconPark', () => {
    expect(source.includes("from 'lucide-react'")).toBe(true);
    expect(source.includes('ChevronDown')).toBe(true);
    expect(source.includes('@icon-park/react')).toBe(false);
  });
});

describe('inlineDiffModel', () => {
  test('builds insert and delete hunks from old/new text', () => {
    const hunks = hunksFromOldNew('const a = 1;\n', 'const a = 2;\n');
    const stats = countDiffStats(hunks);
    expect(stats.deletions).toBe(1);
    expect(stats.insertions).toBe(1);
    expect(countDiffLines(hunks)).toBeGreaterThan(0);
    expect(hunks[0]?.lines.some((line) => line.kind === 'delete' && line.text.includes('const a = 1;'))).toBe(true);
    expect(hunks[0]?.lines.some((line) => line.kind === 'insert' && line.text.includes('const a = 2;'))).toBe(true);
  });

  test('parses unified diffs into the same hunk model', () => {
    const hunks = hunksFromUnifiedDiff(`--- a/file.ts
+++ b/file.ts
@@ -1,1 +1,1 @@
-old
+new
`);
    expect(countDiffStats(hunks)).toEqual({ insertions: 1, deletions: 1 });
  });
});
