import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const css = readFileSync(new URL('./quiet-chrome.css', import.meta.url), 'utf8');

describe('canvas chrome token dark contrast', () => {
  test('studio chrome follows app theme; canvas shells keep an independent light/dark chrome', () => {
    expect(css.includes("[data-theme='dark'] .canvas-chrome-token")).toBe(true);
    expect(css.includes("body[arco-theme='dark'] .canvas-chrome-token")).toBe(true);
    expect(css.includes('.oc-canvas:not(.dark) .canvas-chrome-token')).toBe(true);
    expect(css.includes('.oc-canvas.dark .canvas-chrome-token')).toBe(true);

    const appDarkRule =
      css.match(
        /\[data-theme='dark'\] \.canvas-chrome-token,\s*body\[arco-theme='dark'\] \.canvas-chrome-token \{[\s\S]*?\n\}/
      )?.[0] ?? '';
    expect(appDarkRule.length).toBeGreaterThan(0);
    expect(appDarkRule.includes('--workspace-surface-strong')).toBe(false);
    expect(appDarkRule.includes('--color-bg-2')).toBe(true);
    expect(appDarkRule.includes('--color-text-1')).toBe(true);

    const lightCanvasRule =
      css.match(/\.oc-canvas:not\(\.dark\) \.canvas-chrome-token \{[\s\S]*?\n\}/)?.[0] ?? '';
    expect(lightCanvasRule.length).toBeGreaterThan(0);
    expect(lightCanvasRule.includes('--workspace-surface-strong')).toBe(true);

    const darkCanvasRule =
      css.match(/\.oc-canvas\.dark \.canvas-chrome-token \{[\s\S]*?\n\}/)?.[0] ?? '';
    expect(darkCanvasRule.length).toBeGreaterThan(0);
    expect(darkCanvasRule.includes('--workspace-surface-strong')).toBe(true);
  });

  test('canvas title and chrome icons use canvas text color, not app --bg-base / --color-text-1', () => {
    expect(css.includes(".oc-canvas[data-canvas-theme='light'] .canvas-title-label")).toBe(true);
    expect(css.includes(".oc-canvas[data-canvas-theme='dark'] .canvas-title-label")).toBe(true);
    expect(css.includes('.oc-canvas:not(.dark) .canvas-title-label')).toBe(true);
    expect(css.includes('.oc-canvas.dark .canvas-title-label')).toBe(true);
    expect(css.includes('.oc-canvas:not(.dark) .canvas-title-input')).toBe(true);
    expect(css.includes('.oc-canvas.dark .canvas-title-input')).toBe(true);

    const lightTitleRule =
      css.match(
        /\.oc-canvas\[data-canvas-theme='light'\] \.canvas-title-label,[\s\S]*?\{[\s\S]*?\n\}/
      )?.[0] ?? '';
    expect(lightTitleRule.includes('#111827')).toBe(true);
    expect(lightTitleRule.includes('--color-text-1')).toBe(false);
    expect(lightTitleRule.includes('--bg-base')).toBe(false);

    const darkTitleRule =
      css.match(
        /\.oc-canvas\[data-canvas-theme='dark'\] \.canvas-title-label,[\s\S]*?\{[\s\S]*?\n\}/
      )?.[0] ?? '';
    expect(darkTitleRule.includes('#ffffff')).toBe(true);
    expect(darkTitleRule.includes('--color-text-1')).toBe(false);
    expect(darkTitleRule.includes('--bg-base')).toBe(false);
    expect(css.indexOf("[data-canvas-theme='dark'] .canvas-title-label")).toBeGreaterThan(
      css.indexOf("[data-canvas-theme='light'] .canvas-title-label"),
    );
  });

  test('model pickers follow the canvas shell rather than the outer studio root', () => {
    expect(css.includes('.oc-canvas:not(.dark) .canvas-composer-model-picker')).toBe(true);
    expect(css.includes('.oc-canvas.dark .canvas-composer-model-picker')).toBe(true);
    expect(css.includes('.oc-canvas:not(.dark) .canvas-model-picker-option')).toBe(true);
    expect(css.includes('.oc-canvas.dark .canvas-model-picker-option')).toBe(true);
    expect(css.includes('.oc-canvas:not(.dark) .creation-model-picker.canvas-composer-model-picker')).toBe(true);
    expect(css.includes('.oc-canvas.dark .creation-model-picker.canvas-composer-model-picker')).toBe(true);
  });
});
