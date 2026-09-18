import { describe, expect, test } from 'bun:test';
import {
  STUDIO_MAIN_MIN_WIDTH,
  STUDIO_SESSION_WIDTH_MAX,
  STUDIO_SESSION_WIDTH_MIN,
  STUDIO_SESSION_WIDTH_RATIO_DEFAULT,
  clampStudioSessionWidth,
  computeStudioSessionWidth,
} from './sessionPanelStorage';

describe('computeStudioSessionWidth', () => {
  test('stays near 360 at a compact desktop shell', () => {
    expect(computeStudioSessionWidth(1280, STUDIO_SESSION_WIDTH_RATIO_DEFAULT)).toBe(360);
  });

  test('grows when the studio shell is maximized', () => {
    const wide = computeStudioSessionWidth(1920, STUDIO_SESSION_WIDTH_RATIO_DEFAULT);
    expect(wide).toBeGreaterThan(360);
    expect(wide).toBeLessThanOrEqual(STUDIO_SESSION_WIDTH_MAX);
  });

  test('gives leftover width to the main column first', () => {
    const shell = 1100;
    const width = computeStudioSessionWidth(shell, 0.5);
    expect(shell - width).toBeGreaterThanOrEqual(STUDIO_MAIN_MIN_WIDTH);
    expect(width).toBeGreaterThanOrEqual(STUDIO_SESSION_WIDTH_MIN);
  });

  test('does not exceed the session max on ultra-wide shells', () => {
    expect(computeStudioSessionWidth(2560, 0.4)).toBe(STUDIO_SESSION_WIDTH_MAX);
  });
});

describe('clampStudioSessionWidth', () => {
  test('caps a drag against the main-column floor', () => {
    expect(clampStudioSessionWidth(900, 1200)).toBe(1200 - STUDIO_MAIN_MIN_WIDTH);
  });
});
