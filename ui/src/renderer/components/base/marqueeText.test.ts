import { describe, expect, test } from 'bun:test';

import {
  getMarqueeTotalDuration,
  getMarqueeTravelDuration,
  MARQUEE_MAX_TRAVEL_MS,
  MARQUEE_MIN_TRAVEL_MS,
  MARQUEE_RETURN_MS,
  MARQUEE_END_PAUSE_MS,
  MARQUEE_EASE,
} from './marqueeTextTiming';

describe('marqueeText timing', () => {
  test('does not schedule motion for empty or non-overflowing content', () => {
    expect(getMarqueeTravelDuration(0)).toBe(0);
    expect(getMarqueeTravelDuration(-1)).toBe(0);
    expect(getMarqueeTotalDuration(0)).toBe(0);
  });

  test('uses a readable minimum travel duration for short overflow', () => {
    expect(getMarqueeTravelDuration(1)).toBe(MARQUEE_MIN_TRAVEL_MS);
    expect(getMarqueeTotalDuration(1)).toBe(
      MARQUEE_MIN_TRAVEL_MS + MARQUEE_END_PAUSE_MS + MARQUEE_RETURN_MS
    );
  });

  test('scales with distance and clamps very long content', () => {
    expect(getMarqueeTravelDuration(64)).toBe(2000);
    expect(getMarqueeTravelDuration(100_000)).toBe(MARQUEE_MAX_TRAVEL_MS);
    expect(getMarqueeTotalDuration(100_000)).toBe(
      MARQUEE_MAX_TRAVEL_MS + MARQUEE_END_PAUSE_MS + MARQUEE_RETURN_MS
    );
  });

  test('rejects non-finite distances', () => {
    expect(getMarqueeTravelDuration(Number.NaN)).toBe(0);
    expect(getMarqueeTravelDuration(Number.POSITIVE_INFINITY)).toBe(0);
  });

  test('uses linear timing for uniform travel speed', () => {
    expect(MARQUEE_EASE).toBe('linear');
  });
});
