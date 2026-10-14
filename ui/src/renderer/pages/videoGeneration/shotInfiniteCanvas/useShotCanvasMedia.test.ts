import { describe, expect, test } from 'bun:test';

import { retainShotMediaUrls, uniqueShotMediaPaths } from './useShotCanvasMedia';

describe('uniqueShotMediaPaths', () => {
  test('drops empties, normalizes slashes, and keeps first-seen order', () => {
    expect(
      uniqueShotMediaPaths([
        undefined,
        'refs\\portrait.png',
        '',
        'refs/portrait.png',
        'takes/3.mp4',
        null,
        'takes/3.mp4',
      ])
    ).toEqual(['refs/portrait.png', 'takes/3.mp4']);
  });
});

describe('retainShotMediaUrls', () => {
  test('keeps overlapping paths so a shot switch can paint shared refs immediately', () => {
    const current = {
      'refs/portrait.png': 'blob:portrait',
      'takes/1.mp4': 'blob:old-take',
    };
    const next = retainShotMediaUrls(current, ['refs/portrait.png', 'takes/2.mp4']);
    expect(next).toEqual({ 'refs/portrait.png': 'blob:portrait' });
  });

  test('returns the same object when nothing was pruned', () => {
    const current = { 'refs/portrait.png': 'blob:portrait' };
    expect(retainShotMediaUrls(current, ['refs/portrait.png'])).toBe(current);
  });
});
