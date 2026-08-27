import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./MobileActionSheet.tsx', import.meta.url), 'utf8');
const css = readFileSync(new URL('./MobileActionSheet.module.css', import.meta.url), 'utf8');

describe('MobileActionSheet structure', () => {
  test('keeps the two-pane portal while managing keyboard focus lifecycle', () => {
    expect(source.includes('createPortal')).toBe(true);
    expect(source.includes('document.body')).toBe(true);
    expect(source.includes('previouslyFocusedRef')).toBe(true);
    expect(source.includes('getFocusableElements')).toBe(true);
    expect(source.includes("event.key === 'Escape'")).toBe(true);
    expect(source.includes("event.key !== 'Tab'")).toBe(true);
    expect(source.includes('focus({ preventScroll: true })')).toBe(true);
    expect(source.includes("REDUCED_MOTION_QUERY = '(prefers-reduced-motion: reduce)'")).toBe(true);
    expect(source.includes('prefersReducedMotion()')).toBe(true);
    expect(source.includes("aria-modal='true'")).toBe(true);
    expect(source.includes('aria-labelledby={title ? sheetTitleId : undefined}')).toBe(true);
    expect(source.includes('tabIndex={-1}')).toBe(true);
    expect(css.includes('min-height: 44px')).toBe(true);
    expect(css.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
  });
});
