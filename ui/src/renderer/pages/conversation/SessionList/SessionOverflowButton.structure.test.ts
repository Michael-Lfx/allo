import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('./SessionOverflowButton.tsx', import.meta.url), 'utf8');
const layoutCss = readFileSync(new URL('../../../styles/layout.css', import.meta.url), 'utf8');

describe('SessionOverflowButton structure', () => {
  test('uses an accessible full-row semantic control with count-aware copy', () => {
    expect(source.includes("type='button'")).toBe(true);
    expect(source.includes('aria-expanded={expanded}')).toBe(true);
    expect(source.includes('aria-controls={controlsId}')).toBe(true);
    expect(source.includes("t('sessionList.expandDisplay', { count: hiddenCount })")).toBe(true);
    expect(source.includes("t('sessionList.collapseDisplay')")).toBe(true);
    expect(source.includes('if (hiddenCount <= 0) return null;')).toBe(true);
    expect(source.includes("classNames('session-overflow-button', className)")).toBe(true);
    expect(layoutCss.includes('width: 100%;')).toBe(true);
    expect(layoutCss.includes('min-height: 34px;')).toBe(true);
    expect(layoutCss.includes('width: calc(100% - 42px)')).toBe(false);
    expect(layoutCss.includes('margin: 1px 0 2px 42px')).toBe(false);
  });
});
