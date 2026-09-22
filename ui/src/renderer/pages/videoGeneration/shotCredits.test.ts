import { describe, expect, test } from 'bun:test';
import {
  creditsByShotFromSessionEvents,
  parseShotCreditsFile,
  resolveShotCreditsConsumed,
  shotCreditKey,
} from './shotCredits';

describe('shotCreditKey', () => {
  test('is scene-aware only when the root encodes a scene index', () => {
    expect(shotCreditKey('script2video', 2)).toBe('s:2');
    expect(shotCreditKey('idea2video/scene_1', 0)).toBe('c:1:s:0');
    expect(shotCreditKey(undefined, null)).toBeNull();
  });
});

describe('creditsByShotFromSessionEvents', () => {
  test('attributes a video_credits event to the preceding clip start', () => {
    const map = creditsByShotFromSessionEvents([
      {
        stage: 'video_clip_start',
        message: 'Generating shot 0 video',
        metadata: { shot_idx: 0 },
      },
      {
        stage: 'video_credits',
        message: 'credits 2000',
        metadata: { task_id: 10, credits_consumed: 2000 },
      },
      {
        stage: 'video_clip_start',
        message: 'Generating shot 1 video',
        metadata: { shot_idx: 1 },
      },
      {
        stage: 'video_credits',
        message: 'credits 3200',
        metadata: { task_id: 11, credits_consumed: 3200 },
      },
    ]);
    expect(map.get('s:0')).toBe(2000);
    expect(map.get('s:1')).toBe(3200);
  });

  test('prefers shot_idx on the credits event itself', () => {
    const map = creditsByShotFromSessionEvents([
      {
        stage: 'video_clip_start',
        message: 'Generating shot 0 video',
        metadata: { shot_idx: 0 },
      },
      {
        stage: 'video_credits',
        message: 'credits 1800',
        metadata: { task_id: 12, credits_consumed: 1800, shot_idx: 4, scene_idx: 1 },
      },
    ]);
    expect(map.get('c:1:s:4')).toBe(1800);
    expect(map.has('s:0')).toBe(false);
  });

  test('ignores poll snapshots that are not video_credits', () => {
    const map = creditsByShotFromSessionEvents([
      {
        stage: 'video_clip_start',
        message: 'Generating shot 2 video',
        metadata: { shot_idx: 2 },
      },
      {
        stage: 'video_poll',
        message: 'elapsed 12s',
        metadata: { task_id: 1, credits_consumed: 9999, shot_idx: 2 },
      },
    ]);
    expect(map.size).toBe(0);
  });
});

describe('parseShotCreditsFile', () => {
  test('reads a positive credits_consumed field', () => {
    expect(parseShotCreditsFile('{"credits_consumed":3200,"task_id":9}')).toBe(3200);
    expect(parseShotCreditsFile('{"credits_consumed":0}')).toBe(0);
    expect(parseShotCreditsFile('not-json')).toBe(0);
  });
});

describe('resolveShotCreditsConsumed', () => {
  test('only surfaces credits after the clip exists, taking the max of event and sidecar', () => {
    const eventCredits = new Map([['s:0', 2000]]);
    const sidecarCredits = new Map([['s:0', 2400]]);
    expect(
      resolveShotCreditsConsumed({
        sceneRoot: 'script2video',
        shotIndex: 0,
        hasVideo: false,
        eventCredits,
        sidecarCredits,
      })
    ).toBe(0);
    expect(
      resolveShotCreditsConsumed({
        sceneRoot: 'script2video',
        shotIndex: 0,
        hasVideo: true,
        eventCredits,
        sidecarCredits,
      })
    ).toBe(2400);
  });
});
