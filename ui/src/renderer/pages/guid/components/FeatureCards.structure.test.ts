import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relative: string) => readFileSync(new URL(relative, import.meta.url), 'utf8');
const cards = read('./FeatureCards.tsx');
const css = read('./FeatureCards.module.css');

describe('FeatureCards SVG morph boundaries', () => {
  test('keeps one fixed window and two content states per card', () => {
    expect(cards).toContain('const WINDOW_FRAME_PATH');
    expect(cards).toContain('pathLength={1}');
    expect(cards).toContain('artStateCode');
    expect(cards).toContain('artStatePage');
    expect(cards).toContain('artStateLine');
    expect(cards).toContain('artStateBars');
    expect(cards).toContain('const CREATE_BARS');
    expect(cards).toContain("data-art='build'");
    expect(cards).toContain("data-art='create'");
  });

  test('uses an 800ms reveal with an 800ms bar-wave finish', () => {
    expect(css).toContain('--feature-hover-duration: 800ms');
    expect(css).toContain('--feature-bar-duration: 555ms');
    expect(cards).toContain('index * 35');
    expect(css).toContain(':focus-visible .artStatePage');
    expect(css).toContain('clip-path: inset(0 100% 0 0)');
    expect(css).toContain('transform: scaleY(1)');
  });

  test('does not animate the fixed frame or content layers with translation', () => {
    expect(css).not.toContain('translate(');
    expect(css).not.toContain('translate3d(');
    expect(css).toContain('transform: scaleY(0)');
  });
});
