import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import { COLOR_LAB_IDS, getColorLabTokens } from './palettes';

const pageSource = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');
const routerSource = readFileSync(new URL('../../components/layout/Router.tsx', import.meta.url), 'utf8');

describe('color lab prototype', () => {
  test('registers the hidden test route outside ProtectedLayout', () => {
    expect(routerSource.includes("path='/test/color-lab'")).toBe(true);
    expect(routerSource.includes("import('@renderer/pages/colorLab')")).toBe(true);

    const labIndex = routerSource.indexOf("path='/test/color-lab'");
    const protectedIndex = routerSource.indexOf('element={<ProtectedLayout');
    expect(labIndex).toBeGreaterThan(-1);
    expect(protectedIndex).toBeGreaterThan(-1);
    expect(labIndex).toBeLessThan(protectedIndex);
  });

  test('keeps the five directions and a live product-shell preview', () => {
    expect(COLOR_LAB_IDS).toEqual(['now', 'forest', 'cobalt', 'inkTan', 'olive']);
    expect(pageSource.includes('<ColorLabShell')).toBe(true);
    expect(pageSource.includes("useState<ColorLabId>('forest')")).toBe(true);
    expect(pageSource.includes('compare')).toBe(true);
  });

  test('owns the locked app viewport as its own scrollport', () => {
    const css = readFileSync(new URL('./colorLab.module.css', import.meta.url), 'utf8');
    expect(css.includes('position: fixed')).toBe(true);
    expect(css.includes('overflow-y: auto')).toBe(true);
  });

  test('defines light and dark tokens for every direction', () => {
    for (const id of COLOR_LAB_IDS) {
      const light = getColorLabTokens(id, 'light');
      const dark = getColorLabTokens(id, 'dark');
      expect(light.accent).not.toBe(light.canvas);
      expect(dark.canvas).not.toBe(light.canvas);
      expect(light.accentFg).not.toBe(light.accent);
      expect(dark.accentFg).not.toBe(dark.accent);
    }
    expect(getColorLabTokens('forest', 'light').accent).toBe('#0f766e');
  });
});
