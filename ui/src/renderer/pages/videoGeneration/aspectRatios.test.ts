import { describe, expect, test } from 'bun:test';
import { aspectRatioNumber, normalizeSeedanceAspectRatio } from './aspectRatios';

describe('aspectRatioNumber', () => {
  test('converts Seedance labels to unitless width/height', () => {
    expect(aspectRatioNumber('16:9')).toBeCloseTo(16 / 9);
    expect(aspectRatioNumber('9:16')).toBeCloseTo(9 / 16);
    expect(aspectRatioNumber('1:1')).toBe(1);
  });

  test('falls back to 16:9 for empty or unknown values', () => {
    expect(normalizeSeedanceAspectRatio('')).toBe('16:9');
    expect(aspectRatioNumber('')).toBeCloseTo(16 / 9);
    expect(aspectRatioNumber('not-a-ratio')).toBeCloseTo(16 / 9);
  });
});
