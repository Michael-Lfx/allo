import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./ContextUsageRing.tsx', import.meta.url), 'utf8');

describe('ContextUsageRing structure', () => {
  test('uses a body-mounted, parent-controllable popup with a stable icon footprint', () => {
    expect(source.includes('popupVisible?: boolean')).toBe(true);
    expect(source.includes('onPopupVisibleChange?: (visible: boolean) => void')).toBe(true);
    expect(source.includes('getPopupContainer={() => document.body}')).toBe(true);
    expect(source.includes('useId')).toBe(true);
    expect(source.includes('aria-controls={popupId}')).toBe(true);
    expect(source.includes("data-popup-open={visible ? 'true' : undefined}")).toBe(true);
    expect(source.includes("data-layout-part='context-ring'")).toBe(true);
    expect(source.includes('transition-transform')).toBe(false);
    expect(source.includes('hover:scale-105')).toBe(false);
    expect(source.includes('active:scale-95')).toBe(false);
  });
});
