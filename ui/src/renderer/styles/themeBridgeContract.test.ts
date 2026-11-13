import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const arcoOverride = readFileSync(new URL('./arco-override.css', import.meta.url), 'utf8');
const visualSystem = readFileSync(new URL('./flowy-visual-system.css', import.meta.url), 'utf8');
const arcoPrimaryHook = readFileSync(
  new URL('../hooks/ui/useArcoPrimaryColor.ts', import.meta.url),
  'utf8',
);

const LEGACY_HARDCODED_BUTTON_COLORS = [
  '#86909c',
  '#d3d4d9',
  '#2b2f36',
  '#1f2128',
  '#52525b',
  '#71717a',
];

describe('Arco ↔ Flowy theme bridge', () => {
  test('Arco button overrides resolve through semantic tokens, not hardcoded grays', () => {
    for (const legacy of LEGACY_HARDCODED_BUTTON_COLORS) {
      expect(arcoOverride.includes(legacy)).toBe(false);
    }
    expect(arcoOverride.includes('var(--flowy-border-structural)')).toBe(true);
    expect(arcoOverride.includes('var(--flowy-text-secondary)')).toBe(true);
    expect(arcoOverride.includes('var(--flowy-ink-button)')).toBe(true);
    expect(arcoOverride.includes('var(--flowy-ink-button-fg)')).toBe(true);
    expect(arcoOverride.includes('var(--flowy-shadow-raised)')).toBe(true);
  });

  test('light and dark expose the ink/focus tokens the Arco bridge depends on', () => {
    expect(visualSystem.includes('--flowy-ink-button: #1d1d1f;')).toBe(true);
    expect(visualSystem.includes('--flowy-ink-button: #f5f5f7;')).toBe(true);
    expect(visualSystem.includes("[data-theme='dark']")).toBe(true);
    expect(visualSystem.includes('--flowy-border-structural:')).toBe(true);
    expect(visualSystem.includes('--flowy-shadow-structure:')).toBe(true);
    expect(visualSystem.includes('--flowy-shadow-raised:')).toBe(true);
    expect(visualSystem.includes('--flowy-focus: var(--control-focus-ring, var(--flowy-focus-ring));')).toBe(
      true,
    );
  });

  test('custom-theme updates re-read the applied token instead of caching a palette', () => {
    expect(arcoPrimaryHook.includes("getPropertyValue('--flowy-ink-button')")).toBe(true);
    expect(arcoPrimaryHook.includes("'theme.custom-css.updated'")).toBe(true);
    expect(arcoPrimaryHook.includes('colorScheme')).toBe(true);
  });
});
