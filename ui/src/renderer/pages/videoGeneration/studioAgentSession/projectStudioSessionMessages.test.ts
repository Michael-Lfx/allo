import { describe, expect, test } from 'bun:test';
import {
  projectStudioSessionMessages,
  resolveStudioComposerAction,
} from './projectStudioSessionMessages';
import type { ArtifactNode, SessionStatus } from '../types';

const portraitTree: ArtifactNode[] = [
  {
    name: 'character_portraits',
    path: 'character_portraits',
    is_dir: true,
    children: [
      {
        name: '0_Alice',
        path: 'character_portraits/0_Alice',
        is_dir: true,
        children: [
          {
            name: 'Alice_three_view.png',
            path: 'character_portraits/0_Alice/Alice_three_view.png',
            is_dir: false,
          },
          {
            name: 'Alice_raw.png',
            path: 'character_portraits/0_Alice/Alice_raw.png',
            is_dir: false,
          },
        ],
      },
    ],
  },
];

describe('resolveStudioComposerAction', () => {
  test('busy runs expose stop', () => {
    expect(
      resolveStudioComposerAction({
        busy: true,
        isFailed: false,
        isAction: false,
        hasStoryboard: false,
        hasFinalVideo: false,
        actionAssetsReady: false,
        canRender: false,
      })
    ).toBe('stop');
  });

  test('planned storyboard waits for a render send', () => {
    expect(
      resolveStudioComposerAction({
        busy: false,
        isFailed: false,
        isAction: false,
        hasStoryboard: true,
        hasFinalVideo: false,
        actionAssetsReady: false,
        canRender: true,
      })
    ).toBe('render');
  });

  test('a finished film hides send', () => {
    expect(
      resolveStudioComposerAction({
        busy: false,
        isFailed: false,
        isAction: false,
        hasStoryboard: true,
        hasFinalVideo: true,
        actionAssetsReady: false,
        canRender: false,
      })
    ).toBe('none');
  });

  test('failed or interrupted runs expose continue', () => {
    expect(
      resolveStudioComposerAction({
        busy: false,
        isFailed: true,
        isAction: false,
        hasStoryboard: true,
        hasFinalVideo: false,
        actionAssetsReady: false,
        canRender: true,
      })
    ).toBe('continue');
  });
});

describe('projectStudioSessionMessages', () => {
  test('opens with the user brief', () => {
    const messages = projectStudioSessionMessages({
      sourceText: 'A rainy alley fight.',
      artifacts: [],
      hasStoryboard: false,
      hasFinalVideo: false,
      isAction: false,
    });
    expect(messages[0]).toMatchObject({ kind: 'user_brief', role: 'user' });
    expect(messages[0].text).toBe('A rainy alley fight.');
  });

  test('attaches home uploads onto the user brief', () => {
    const messages = projectStudioSessionMessages({
      sourceText: 'A rainy alley fight.',
      artifacts: [],
      hasStoryboard: false,
      hasFinalVideo: false,
      isAction: false,
      briefMedia: [
        {
          id: 'cameo:a',
          kind: 'image',
          path: 'a',
          label: 'Hero',
          origin: 'cameo',
        },
        { id: 'doc:script.md', kind: 'file', path: 'script.md', label: 'script.md' },
      ],
    });
    expect(messages[0].media?.map((m) => m.id)).toEqual(['cameo:a', 'doc:script.md']);
  });

  test('collapses planning stages into one live bubble', () => {
    const status: SessionStatus = {
      stage: 'write_script',
      message: '',
      progress: 12,
      status: 'planning',
      events: [
        { stage: 'planning', message: '', at: '2026-08-18T01:00:00.000Z' },
        { stage: 'develop_story', message: '', at: '2026-08-18T01:00:08.000Z' },
        { stage: 'write_script', message: '', at: '2026-08-18T01:00:20.000Z' },
      ],
    };
    const messages = projectStudioSessionMessages({
      sourceText: 'idea',
      status,
      artifacts: [],
      hasStoryboard: false,
      hasFinalVideo: false,
      isAction: false,
    });
    const milestones = messages.filter((m) => m.kind === 'milestone');
    expect(milestones).toHaveLength(1);
    expect(milestones[0].beat).toBe('plan');
    expect(milestones[0].stage).toBe('write_script');
    expect(milestones[0].live).toBe(true);
  });

  test('video_poll heartbeats update the clip bubble instead of appending', () => {
    const status: SessionStatus = {
      stage: 'video_poll',
      message: '',
      progress: 40,
      status: 'rendering',
      events: [
        { stage: 'render_start', message: '', at: 'a' },
        { stage: 'video_clips_start', message: '', at: 'b' },
        { stage: 'video_poll', message: '', at: 'c', metadata: { elapsed_secs: 12 } },
        { stage: 'video_poll', message: '', at: 'd', metadata: { elapsed_secs: 40 } },
      ],
    };
    const messages = projectStudioSessionMessages({
      artifacts: [],
      status,
      hasStoryboard: true,
      hasFinalVideo: false,
      isAction: false,
    });
    const clips = messages.filter((m) => m.beat === 'render_clips');
    expect(clips).toHaveLength(1);
    expect(clips[0].pollWaitSecs).toBe(40);
    expect(clips[0].live).toBe(true);
  });

  test('adds a render gate after planning finishes', () => {
    const status: SessionStatus = {
      stage: 'planned',
      message: '',
      progress: 100,
      status: 'idle',
      events: [
        { stage: 'planning', message: '', at: 'a' },
        { stage: 'planned', message: '', at: 'b' },
      ],
    };
    const messages = projectStudioSessionMessages({
      artifacts: [],
      status,
      hasStoryboard: true,
      hasFinalVideo: false,
      isAction: false,
    });
    expect(messages.some((m) => m.kind === 'gate_render')).toBe(true);
  });

  test('attaches portrait images to the portraits beat', () => {
    const status: SessionStatus = {
      stage: 'character_portraits_done',
      message: '',
      progress: 50,
      status: 'planning',
      events: [
        { stage: 'character_portraits_start', message: '', at: 'a' },
        { stage: 'character_portraits_done', message: '', at: 'b' },
      ],
    };
    const messages = projectStudioSessionMessages({
      artifacts: portraitTree,
      status,
      hasStoryboard: false,
      hasFinalVideo: false,
      isAction: false,
    });
    const portraits = messages.find((m) => m.beat === 'portraits');
    expect(portraits?.media?.map((m) => m.path)).toEqual([
      'character_portraits/0_Alice/Alice_three_view.png',
    ]);
  });

  test('surfaces a failure bubble', () => {
    const status: SessionStatus = {
      stage: 'failed',
      message: '',
      progress: 0,
      status: 'failed',
      error: 'llm failed',
      events: [{ stage: 'write_script', message: '', at: 'a' }, { stage: 'failed', message: '', at: 'b' }],
    };
    const messages = projectStudioSessionMessages({
      artifacts: [],
      status,
      hasStoryboard: false,
      hasFinalVideo: false,
      isAction: false,
    });
    expect(messages.some((m) => m.kind === 'failure' && m.error === 'llm failed')).toBe(true);
    expect(messages.some((m) => m.kind === 'gate_render')).toBe(false);
  });

  test('render recap of planning stages does not append a second portraits bubble', () => {
    const status: SessionStatus = {
      stage: 'video_poll',
      message: '',
      progress: 62,
      status: 'rendering',
      events: [
        { stage: 'extract_characters', message: '', at: 'a' },
        { stage: 'character_portraits_start', message: '', at: 'b' },
        { stage: 'character_portraits_done', message: '', at: 'c' },
        { stage: 'world_assets_start', message: '', at: 'd' },
        { stage: 'planned', message: '', at: 'e' },
        { stage: 'reuse_plan', message: '', at: 'f' },
        { stage: 'character_portraits_start', message: '', at: 'g' },
        { stage: 'frames_done', message: '', at: 'h' },
        { stage: 'video_poll', message: '', at: 'i', metadata: { elapsed_secs: 9 } },
      ],
    };
    const messages = projectStudioSessionMessages({
      artifacts: [],
      status,
      hasStoryboard: true,
      hasFinalVideo: false,
      isAction: false,
    });
    const milestones = messages.filter((m) => m.kind === 'milestone');
    expect(milestones.map((m) => m.beat)).toEqual([
      'plan',
      'portraits',
      'world',
      'storyboard',
      'render_frames',
      'render_clips',
    ]);
    const portraits = milestones.filter((m) => m.beat === 'portraits');
    expect(portraits).toHaveLength(1);
    expect(portraits[0].stage).toBe('character_portraits_done');
    expect(portraits[0].live).toBe(false);
    const storyboard = milestones.find((m) => m.beat === 'storyboard');
    expect(storyboard?.stage).toBe('planned');
    const live = milestones.filter((m) => m.live);
    expect(live).toHaveLength(1);
    expect(live[0].beat).toBe('render_clips');
    expect(live[0].pollWaitSecs).toBe(9);
  });

  test('action assets ready get a generate gate', () => {
    const status: SessionStatus = {
      stage: 'planned',
      message: '',
      progress: 100,
      status: 'idle',
      events: [{ stage: 'action_prepare', message: '', at: 'a' }],
    };
    const messages = projectStudioSessionMessages({
      artifacts: [],
      status,
      hasStoryboard: false,
      hasFinalVideo: false,
      isAction: true,
      actionAssetsReady: true,
    });
    expect(messages.some((m) => m.kind === 'gate_action')).toBe(true);
    expect(messages.some((m) => m.kind === 'gate_render')).toBe(false);
  });

  test('rebuilds chapter history from artifacts when the event log is empty', () => {
    const artifacts: ArtifactNode[] = [
      ...portraitTree,
      { name: 'look_plate.png', path: 'look_plate.png', is_dir: false },
      {
        name: 'environments',
        path: 'environments',
        is_dir: true,
        children: [
          {
            name: 'alley_environment_plate.png',
            path: 'environments/alley_environment_plate.png',
            is_dir: false,
          },
        ],
      },
      {
        name: 'shots',
        path: 'script2video/shots',
        is_dir: true,
        children: [
          {
            name: '0',
            path: 'script2video/shots/0',
            is_dir: true,
            children: [
              {
                name: 'first_frame.png',
                path: 'script2video/shots/0/first_frame.png',
                is_dir: false,
              },
              { name: 'video.mp4', path: 'script2video/shots/0/video.mp4', is_dir: false },
            ],
          },
        ],
      },
    ];
    const messages = projectStudioSessionMessages({
      sourceText: 'A rainy alley fight.',
      artifacts,
      status: {
        stage: 'render_done',
        message: '',
        progress: 100,
        status: 'succeeded',
        events: [],
      },
      hasStoryboard: true,
      hasFinalVideo: true,
      finalVideoPath: 'script2video/final_video.mp4',
      coverPath: 'script2video/cover.png',
      isAction: false,
    });
    expect(messages.filter((m) => m.beat).map((m) => m.beat)).toEqual([
      'plan',
      'portraits',
      'world',
      'storyboard',
      'render_frames',
      'render_clips',
      'film',
    ]);
    expect(messages[0]?.kind).toBe('user_brief');
    expect(messages.at(-1)?.kind).toBe('film_ready');
    const world = messages.find((m) => m.beat === 'world');
    expect(world?.media?.map((m) => m.path)).toEqual([
      'environments/alley_environment_plate.png',
    ]);
    expect(messages.some((m) => m.kind === 'user_brief')).toBe(true);
  });

  test('places film_ready last so the session reads chronologically', () => {
    const messages = projectStudioSessionMessages({
      sourceText: 'A rainy alley fight.',
      artifacts: [],
      status: {
        stage: 'render_done',
        message: '',
        progress: 100,
        status: 'succeeded',
        events: [
          { stage: 'planned', message: '', at: 'a' },
          { stage: 'frames_done', message: '', at: 'b' },
          { stage: 'render_done', message: '', at: 'c' },
        ],
      },
      hasStoryboard: true,
      hasFinalVideo: true,
      finalVideoPath: 'idea2video/final_video.mp4',
      isAction: false,
    });
    expect(messages[0]?.kind).toBe('user_brief');
    expect(messages.at(-1)?.kind).toBe('film_ready');
    const storyboardAt = messages.findIndex((m) => m.beat === 'storyboard');
    const filmAt = messages.findIndex((m) => m.kind === 'film_ready');
    expect(storyboardAt).toBeGreaterThan(0);
    expect(storyboardAt).toBeLessThan(filmAt);
  });

  test('keeps canonical order when resume recaps land after render events', () => {
    const artifacts: ArtifactNode[] = [
      ...portraitTree,
      { name: 'story.txt', path: 'idea2video/story.txt', is_dir: false },
    ];
    const status: SessionStatus = {
      stage: 'video_poll',
      message: '',
      progress: 70,
      status: 'rendering',
      events: [
        { stage: 'frames_done', message: '', at: 'a' },
        { stage: 'video_poll', message: '', at: 'b', metadata: { elapsed_secs: 4 } },
        { stage: 'reuse_plan', message: '', at: 'c' },
        { stage: 'extract_characters', message: '', at: 'd' },
        { stage: 'planned', message: '', at: 'e' },
        { stage: 'character_portraits_start', message: '', at: 'f' },
      ],
    };
    const messages = projectStudioSessionMessages({
      sourceText: 'idea',
      artifacts,
      status,
      hasStoryboard: true,
      hasFinalVideo: false,
      isAction: false,
    });
    const milestones = messages.filter((m) => m.kind === 'milestone');
    expect(milestones.map((m) => m.beat)).toEqual([
      'plan',
      'portraits',
      'storyboard',
      'render_frames',
      'render_clips',
    ]);
    expect(milestones.find((m) => m.beat === 'plan')?.media?.some((item) => item.role === 'story')).toBe(
      true
    );
    expect(milestones.find((m) => m.beat === 'portraits')?.stage).toBe('character_portraits_done');
    expect(milestones.find((m) => m.beat === 'portraits')?.live).toBe(false);
    const live = milestones.filter((m) => m.live);
    expect(live).toHaveLength(1);
    expect(live[0]?.beat).toBe('render_clips');
  });
});
