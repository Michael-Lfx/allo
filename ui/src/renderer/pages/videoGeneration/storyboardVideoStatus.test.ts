import { describe, expect, test } from 'vitest';
import {
  activeVideoGenerationTarget,
  resolveStoryboardVideoStatus,
} from './storyboardVideoStatus';
import type { SessionStatus } from './types';

function status(partial: Partial<SessionStatus>): SessionStatus {
  return {
    stage: 'idle',
    message: '',
    progress: 0,
    status: 'idle',
    ...partial,
  };
}

describe('activeVideoGenerationTarget', () => {
  test('returns null when not rendering', () => {
    expect(
      activeVideoGenerationTarget(
        status({ status: 'idle', stage: 'video_clip_start' })
      )
    ).toEqual({ shotIndex: null, sceneIndex: null });
  });

  test('reads shot_idx metadata from newest clip-start event', () => {
    expect(
      activeVideoGenerationTarget(
        status({
          status: 'rendering',
          stage: 'video_clip_start',
          message: '串行生成镜头视频（1/2）· 镜头 0',
          events: [
            {
              stage: 'video_clip_start',
              message: 'Generating shot 0 video',
              metadata: { shot_idx: 0, progress: 55 },
            },
          ],
        })
      )
    ).toEqual({ shotIndex: 0, sceneIndex: null });
  });

  test('clears shot while clip-done is the current stage', () => {
    expect(
      activeVideoGenerationTarget(
        status({
          status: 'rendering',
          stage: 'video_clip_done',
          message: 'Shot 0 ready',
          events: [
            {
              stage: 'video_clip_start',
              message: 'Generating shot 0 video',
              metadata: { shot_idx: 0 },
            },
            {
              stage: 'video_clip_done',
              message: 'Shot 0 ready',
              metadata: { shot_idx: 0 },
            },
          ],
        })
      )
    ).toEqual({ shotIndex: null, sceneIndex: null });
  });

  test('tracks multi-scene render_scene metadata', () => {
    expect(
      activeVideoGenerationTarget(
        status({
          status: 'rendering',
          stage: 'video_clip_start',
          message: 'Generating shot 1 video',
          events: [
            {
              stage: 'render_scene',
              message: '正在渲染场景（2/2）',
              metadata: { scene_idx: 1 },
            },
            {
              stage: 'video_clip_start',
              message: 'Generating shot 1 video',
              metadata: { shot_idx: 1 },
            },
          ],
        })
      )
    ).toEqual({ shotIndex: 1, sceneIndex: 1 });
  });
});

describe('resolveStoryboardVideoStatus', () => {
  test('ready when video exists', () => {
    expect(
      resolveStoryboardVideoStatus({
        hasVideo: true,
        shotIndex: 0,
        rendering: true,
        target: { shotIndex: 0, sceneIndex: null },
      })
    ).toBe('ready');
  });

  test('generating only for the active shot', () => {
    expect(
      resolveStoryboardVideoStatus({
        hasVideo: false,
        shotIndex: 0,
        rendering: true,
        target: { shotIndex: 0, sceneIndex: null },
      })
    ).toBe('generating');
    expect(
      resolveStoryboardVideoStatus({
        hasVideo: false,
        shotIndex: 1,
        rendering: true,
        target: { shotIndex: 0, sceneIndex: null },
      })
    ).toBe('pending');
  });

  test('respects scene root when scene_idx is known', () => {
    expect(
      resolveStoryboardVideoStatus({
        hasVideo: false,
        shotIndex: 0,
        sceneRoot: 'idea2video/scene_1',
        rendering: true,
        target: { shotIndex: 0, sceneIndex: 1 },
      })
    ).toBe('generating');
    expect(
      resolveStoryboardVideoStatus({
        hasVideo: false,
        shotIndex: 0,
        sceneRoot: 'idea2video/scene_0',
        rendering: true,
        target: { shotIndex: 0, sceneIndex: 1 },
      })
    ).toBe('pending');
  });
});
