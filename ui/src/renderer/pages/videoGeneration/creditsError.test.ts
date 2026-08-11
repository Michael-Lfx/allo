import { describe, expect, test } from 'bun:test';
import { isInsufficientCreditsError } from './creditsError';

describe('isInsufficientCreditsError', () => {
  test('matches Flowy API error 402 with Chinese msg', () => {
    expect(isInsufficientCreditsError('API error 402: 积分不足')).toBe(true);
  });

  test('matches map_model_err hint marker', () => {
    expect(
      isInsufficientCreditsError(
        'Video generation failed\nHint: INSUFFICIENT_CREDITS — Flowy credits are too low.'
      )
    ).toBe(true);
  });

  test('matches English insufficient credit phrasing', () => {
    expect(isInsufficientCreditsError('insufficient credits for video: need 5000')).toBe(true);
  });

  test('ignores unrelated failures', () => {
    expect(isInsufficientCreditsError('Video generation failed — model unavailable')).toBe(false);
    expect(isInsufficientCreditsError('Rate limited — retry shortly.')).toBe(false);
    expect(isInsufficientCreditsError(null)).toBe(false);
  });
});
