import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import { pauseMediaElement, playMediaElement } from './mediaPlayback';

const source = (rel: string) => readFileSync(new URL(rel, import.meta.url), 'utf8');

describe('playMediaElement / pauseMediaElement', () => {
  test('no-ops when play/pause are missing instead of throwing', () => {
    const stub = {};
    expect(() => playMediaElement(stub)).not.toThrow();
    expect(() => pauseMediaElement(stub)).not.toThrow();
    expect(() => playMediaElement(null)).not.toThrow();
    expect(() => pauseMediaElement(undefined)).not.toThrow();
  });

  test('invokes play and swallows rejection', async () => {
    const calls: string[] = [];
    playMediaElement({
      play: () => {
        calls.push('play');
        return Promise.reject(new Error('blocked'));
      },
    });
    expect(calls).toEqual(['play']);
    await Promise.resolve();
  });

  test('invokes pause when it is a function', () => {
    const calls: string[] = [];
    pauseMediaElement({
      pause: () => {
        calls.push('pause');
      },
    });
    expect(calls).toEqual(['pause']);
  });

  test('campaign carousel and home cards never call pause/play raw', () => {
    const carousel = source('./components/CampaignCarousel.tsx');
    const tv = source('./components/TvShowCard.tsx');
    const recent = source('./components/SessionCard.tsx');
    const task = source('./components/GenerationTaskCard.tsx');

    for (const text of [carousel, tv, recent, task]) {
      expect(text).toContain('playMediaElement');
      expect(text).toContain('pauseMediaElement');
      expect(text).not.toMatch(/\.(play|pause)\(/);
    }
  });
});
