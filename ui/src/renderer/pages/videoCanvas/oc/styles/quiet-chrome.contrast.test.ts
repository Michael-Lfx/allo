import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const css = readFileSync(new URL('./quiet-chrome.css', import.meta.url), 'utf8');

describe('canvas chrome token dark contrast', () => {
  test('restyles chrome chips via app theme attributes, not canvas html.dark', () => {
    expect(css.includes("[data-theme='dark'] .canvas-chrome-token")).toBe(true);
    expect(css.includes("body[arco-theme='dark'] .canvas-chrome-token")).toBe(true);

    const darkRule =
      css.match(
        /\[data-theme='dark'\] \.canvas-chrome-token,\s*body\[arco-theme='dark'\] \.canvas-chrome-token \{[\s\S]*?\n\}/
      )?.[0] ?? '';
    expect(darkRule.length).toBeGreaterThan(0);
    expect(darkRule.includes('--workspace-surface-strong')).toBe(false);
    expect(darkRule.includes('--color-bg-2')).toBe(true);
    expect(darkRule.includes('--color-text-1')).toBe(true);

    const darkHoverRule =
      css.match(
        /\[data-theme='dark'\] \.canvas-chrome-token:hover,[\s\S]*?body\[arco-theme='dark'\] \.canvas-chrome-token\[aria-pressed="true"\] \{[\s\S]*?\n\}/
      )?.[0] ?? '';
    expect(darkHoverRule.length).toBeGreaterThan(0);
    expect(darkHoverRule.includes('--workspace-surface-strong')).toBe(false);
    expect(darkHoverRule.includes('--color-bg-2')).toBe(true);
  });
});
