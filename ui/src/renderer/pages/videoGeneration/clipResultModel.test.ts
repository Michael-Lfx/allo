import { describe, expect, test } from 'bun:test';
import {
  clipFollowUp,
  clipFrameAspect,
  clipProgressPercent,
  clipRegenerateBody,
  clipStepKey,
  clipStillIds,
  clipTimestampMs,
  shortClipModel,
} from './clipResultModel';

describe('clip detail view model', () => {
  test('keeps millisecond timestamps and scales unix seconds', () => {
    expect(clipTimestampMs(1_764_000_000_000)).toBe(1_764_000_000_000);
    expect(clipTimestampMs(1_764_000_000)).toBe(1_764_000_000_000);
    expect(clipTimestampMs(0)).toBeNull();
    expect(clipTimestampMs(null)).toBeNull();
  });

  test('uses the decoded video ratio, then the task ratio', () => {
    expect(clipFrameAspect(9 / 16, '16:9')).toBeCloseTo(9 / 16);
    expect(clipFrameAspect(null, '9:16')).toBeCloseTo(9 / 16);
    expect(clipFrameAspect(null, null)).toBeCloseTo(16 / 9);
    expect(clipFrameAspect(0, '1:1')).toBeCloseTo(1);
  });

  test('treats zero progress as unknown and accepts both ratios and percents', () => {
    expect(clipProgressPercent(0)).toBeNull();
    expect(clipProgressPercent(null)).toBeNull();
    expect(clipProgressPercent(0.4)).toBe(40);
    expect(clipProgressPercent(1)).toBe(100);
    expect(clipProgressPercent(40)).toBe(40);
  });

  test('regenerates finished clips and only refreshes in-flight or unpaid ones', () => {
    expect(clipFollowUp('failed')).toBe('regenerate');
    expect(clipFollowUp('canceled')).toBe('regenerate');
    expect(clipFollowUp('succeeded')).toBe('regenerate');
    expect(clipFollowUp('failed', { credits: true })).toBe('resume');
    expect(clipFollowUp('running')).toBe('resume');
    expect(clipFollowUp('queued')).toBe('resume');
    expect(clipFollowUp(null)).toBe('resume');
  });

  test('names the in-flight step from progress', () => {
    expect(clipStepKey('queued', null)).toBe('queued');
    expect(clipStepKey('running', null)).toBe('initializing');
    expect(clipStepKey('running', 40)).toBe('rendering');
    expect(clipStepKey('running', 90)).toBe('finalizing');
  });

  test('copies audio and reference video when generating again', () => {
    expect(
      clipRegenerateBody({
        mode: 'video',
        prompt: '  rain on the platform  ',
        model: 'org/seedance',
        aspect_ratio: '9:16',
        resolution: '720p',
        duration_secs: 5,
        reference_media_ids: ['img-1'],
        audio_media_ids: ['aud-1'],
        reference_video_media_id: 'vid-1',
        first_frame_media_id: 'first',
        last_frame_media_id: 'last',
      })
    ).toEqual({
      mode: 'video',
      prompt: 'rain on the platform',
      model: 'org/seedance',
      aspect_ratio: '9:16',
      resolution: '720p',
      duration_secs: 5,
      reference_media_ids: ['img-1'],
      audio_media_ids: ['aud-1'],
      reference_video_media_id: 'vid-1',
      first_frame_media_id: 'first',
      last_frame_media_id: 'last',
    });
    expect(clipRegenerateBody({ prompt: '   ' })).toBeNull();
    expect(shortClipModel('org/seedance-2')).toBe('seedance-2');
  });

  test('orders stills as first frame, references, last frame without duplicates', () => {
    expect(
      clipStillIds({
        first_frame_media_id: 'first',
        reference_media_ids: ['first', 'ref'],
        last_frame_media_id: 'last',
      })
    ).toEqual(['first', 'ref', 'last']);
  });
});
