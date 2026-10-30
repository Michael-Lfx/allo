

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const css = readFileSync(new URL('./flowy-visual-system.css', import.meta.url), 'utf8');
const direction = readFileSync(
  new URL('../../../../docs/superpowers/design/2026-07-22-flowy-visual-direction.md', import.meta.url),
  'utf8'
);

const REQUIRED_TOKENS = [
  '--flowy-text-display',
  '--flowy-space-1',
  '--flowy-space-2',
  '--flowy-radius-md',
  '--flowy-surface-0',
  '--flowy-surface-1',
  '--flowy-accent',
  '--flowy-success',
  '--flowy-warning',
  '--flowy-danger',
  '--flowy-density-rail',
  '--flowy-density-session',
  '--flowy-density-workspace',
  '--flowy-focus-ring',
  '--flowy-canvas',
  '--flowy-panel',
  '--flowy-raised',
  '--flowy-interactive',
  '--flowy-selected-bg',
  '--flowy-text-primary',
  '--flowy-attention',
  '--flowy-attention-fg',
  '--flowy-border-structural',
  '--flowy-shadow-focus',
];

describe('flowy visual system', () => {
  test('locks Ink Studio as the shipped direction', () => {
    expect(direction.includes('已选：Ink Studio')).toBe(true);
    expect(direction.includes('Warm Atelier（否决）')).toBe(true);
    expect(direction.includes('Quiet Kinetic')).toBe(true);
  });

  test('declares light and dark semantic token blocks', () => {
    for (const token of REQUIRED_TOKENS) {
      expect(css.includes(`${token}:`)).toBe(true);
    }
    expect(css.includes("[data-theme='dark']")).toBe(true);
    expect(css.includes("body[arco-theme='dark']")).toBe(true);
  });

  test('keeps attention styling in the readable steel blue-gray range', () => {
    expect(css.includes('--flowy-attention: oklch(56% 0.055 250)')).toBe(true);
    expect(css.includes('--flowy-attention-fg: oklch(98% 0.01 95)')).toBe(true);
  });


  test('uses the system font stack and concentric control radii', () => {
    expect(css.includes("-apple-system, BlinkMacSystemFont, 'Segoe UI Variable Text'")).toBe(true);
    expect(css.includes('--flowy-text-display: 22px')).toBe(true);
    expect(css.includes('--flowy-text-body: 15px')).toBe(true);
    expect(css.includes('--flowy-text-control: 13px')).toBe(true);
    expect(css.includes('--flowy-radius-control: 6px')).toBe(true);
    expect(css.includes('--flowy-radius-field: 8px')).toBe(true);
    expect(css.includes('html.platform-macos')).toBe(true);
  });

  test('keeps semantic surfaces mapped to the established theme contract', () => {
    expect(css.includes('--flowy-canvas: var(--bg-base')).toBe(true);
    expect(css.includes('--flowy-panel: var(--bg-1')).toBe(true);
    expect(css.includes('--flowy-raised: var(--color-bg-popup')).toBe(true);
  });

  test('declares GitHub-style code token colors for light and dark', () => {
    expect(css.includes('--code-font:')).toBe(true);
    expect(css.includes('--code-token-keyword:')).toBe(true);
    expect(css.includes('--code-bg: #f6f8fa')).toBe(true);
    const darkBlock = css.slice(css.indexOf("[data-theme='dark']"));
    expect(darkBlock.includes('--code-token-keyword: #ff7b72')).toBe(true);
    expect(darkBlock.includes('--code-token-function: #d2a8ff')).toBe(true);
    expect(darkBlock.includes('--code-token-string: #a5d6ff')).toBe(true);
    expect(darkBlock.includes('--code-bg: #0d1117')).toBe(true);
  });
});
