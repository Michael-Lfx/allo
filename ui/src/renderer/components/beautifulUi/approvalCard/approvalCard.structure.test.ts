import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./ApprovalCard.tsx', import.meta.url), 'utf8');
const cssSource = readFileSync(new URL('./approvalCard.module.css', import.meta.url), 'utf8');

describe('ApprovalCard', () => {
  test('covers Beautiful UI approval kinds without inventing message types', () => {
    expect(
      source.includes("export type ApprovalKind = 'edit' | 'exec' | 'info' | 'mcp' | 'plan'")
    ).toBe(true);
    expect(source.includes("data-testid='beautiful-ui-approval-card'")).toBe(true);
    expect(source.includes('const exhaustive: never = kind')).toBe(true);
    expect(cssSource.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
  });

  test('uses Beautiful UI Lucide glyphs instead of IconPark', () => {
    expect(source.includes("from 'lucide-react'")).toBe(true);
    expect(source.includes('Pencil')).toBe(true);
    expect(source.includes('Terminal')).toBe(true);
    expect(source.includes('@icon-park/react')).toBe(false);
  });
});
