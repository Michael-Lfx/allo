

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

import { computeTooltipCoords } from './InstantHoverTooltip';

const source = readFileSync(new URL('./InstantHoverTooltip.tsx', import.meta.url), 'utf8');

describe('InstantHoverTooltip', () => {
  test('renders tooltip through a body portal with fixed positioning', () => {
    expect(source.includes("role='tooltip'")).toBe(true);
    expect(source.includes('createPortal')).toBe(true);
    expect(source.includes('document.body')).toBe(true);
    expect(source.includes('fixed z-[10001]')).toBe(true);
    expect(source.includes('instant-hover-tooltip')).toBe(true);
    expect(source.includes('onMouseEnter={show}')).toBe(true);
    expect(source.includes('onMouseLeave={hide}')).toBe(true);
    expect(source.includes('onFocus={show}')).toBe(true);
    expect(source.includes('onBlur={hide}')).toBe(true);
  });

  test('computeTooltipCoords places bottom tooltips below the anchor', () => {
    const rect = {
      top: 10,
      left: 20,
      right: 44,
      bottom: 34,
      width: 24,
      height: 24,
    } as DOMRect;

    expect(computeTooltipCoords(rect, 'bottom')).toEqual({ top: 40, left: 32 });
    expect(computeTooltipCoords(rect, 'top')).toEqual({ top: 4, left: 32 });
    expect(computeTooltipCoords(rect, 'right')).toEqual({ top: 22, left: 50 });
  });
});
