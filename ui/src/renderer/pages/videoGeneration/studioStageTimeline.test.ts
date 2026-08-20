import { describe, expect, test } from 'bun:test';
import {
  buildStudioStageTimeline,
  macroStageOf,
  studioStageActiveIndex,
} from './studioStageTimeline';

const base = {
  hasStoryboard: false,
  hasFinalVideo: false,
  nowMs: Date.parse('2026-08-18T01:10:00.000Z'),
};

describe('macroStageOf', () => {
  test('maps planning, storyboard, render and film stages', () => {
    expect(macroStageOf('write_script')).toBe('brief');
    expect(macroStageOf('plan_scene')).toBe('storyboard');
    expect(macroStageOf('design_storyboard')).toBe('storyboard');
    expect(macroStageOf('character_portraits_start')).toBe('storyboard');
    expect(macroStageOf('video_poll')).toBe('render');
    expect(macroStageOf('concat_done')).toBe('film');
    expect(macroStageOf('failed')).toBeNull();
  });

  test('action imitation folds its stages into assets / generate / film', () => {
    expect(macroStageOf('action_prepare', 'action')).toBe('assets');
    expect(macroStageOf('planned', 'action')).toBe('assets');
    expect(macroStageOf('action_generate', 'action')).toBe('generate');
    expect(macroStageOf('film_cover_start', 'action')).toBe('generate');
    expect(macroStageOf('render_done', 'action')).toBe('film');
    expect(macroStageOf('design_storyboard', 'action')).toBeNull();
  });
});

describe('studioStageActiveIndex', () => {
  test('is -1 before the run starts', () => {
    expect(studioStageActiveIndex({ ...base, status: 'idle' })).toBe(-1);
  });

  test('a finished film wins over live status', () => {
    expect(studioStageActiveIndex({ ...base, status: 'planning', hasFinalVideo: true })).toBe(3);
  });
});

describe('buildStudioStageTimeline', () => {
  test('every phase is pending and equally weighted before the run starts', () => {
    const segments = buildStudioStageTimeline({ ...base, status: 'idle' });
    expect(segments.map((s) => s.state)).toEqual(['pending', 'pending', 'pending', 'pending']);
    expect(segments.map((s) => s.weight)).toEqual([1, 1, 1, 1]);
  });

  test('phase duration accumulates per macro stage and the live phase keeps ticking', () => {
    const segments = buildStudioStageTimeline({
      ...base,
      status: 'rendering',
      stage: 'video_poll',
      nowMs: Date.parse('2026-08-18T01:05:00.000Z'),
      events: [
        { stage: 'planning', message: '', at: '2026-08-18T01:00:00.000Z' },
        { stage: 'write_script', message: '', at: '2026-08-18T01:00:10.000Z' },
        { stage: 'design_storyboard', message: '', at: '2026-08-18T01:00:30.000Z' },
        { stage: 'render_start', message: '', at: '2026-08-18T01:00:40.000Z' },
        { stage: 'video_poll', message: '', at: '2026-08-18T01:01:00.000Z' },
      ],
    });

    expect(segments[0].durationMs).toBe(30_000);
    expect(segments[1].durationMs).toBe(10_000);
    expect(segments[2].durationMs).toBe(260_000);
    expect(segments[3].durationMs).toBeNull();
    expect(segments.map((s) => s.state)).toEqual(['done', 'done', 'active', 'pending']);
    expect(segments[2].live).toBe(true);
    expect(segments[2].weight).toBeGreaterThan(segments[0].weight);
  });

  test('a failure marks the phase the pipeline was in and leaves later phases pending', () => {
    const segments = buildStudioStageTimeline({
      ...base,
      status: 'failed',
      stage: 'failed',
      updatedAt: '2026-08-18T01:02:00.000Z',
      events: [
        { stage: 'planning', message: '', at: '2026-08-18T01:00:00.000Z' },
        { stage: 'design_storyboard', message: '', at: '2026-08-18T01:00:20.000Z' },
        { stage: 'failed', message: '', at: '2026-08-18T01:01:00.000Z' },
      ],
    });

    expect(segments.map((s) => s.state)).toEqual(['done', 'failed', 'pending', 'pending']);
    expect(segments[1].durationMs).toBe(40_000);
    expect(segments[1].live).toBe(false);
  });

  test('the action variant has three phases weighted by their own stages', () => {
    const segments = buildStudioStageTimeline({
      ...base,
      variant: 'action',
      status: 'rendering',
      stage: 'action_generate',
      nowMs: Date.parse('2026-08-18T01:02:00.000Z'),
      events: [
        { stage: 'action_prepare', message: '', at: '2026-08-18T01:00:00.000Z' },
        { stage: 'planned', message: '', at: '2026-08-18T01:00:05.000Z' },
        { stage: 'action_generate', message: '', at: '2026-08-18T01:00:10.000Z' },
      ],
    });

    expect(segments.map((s) => s.key)).toEqual(['assets', 'generate', 'film']);
    expect(segments[0].durationMs).toBe(10_000);
    expect(segments[1].durationMs).toBe(110_000);
    expect(segments.map((s) => s.state)).toEqual(['done', 'active', 'pending']);
    expect(segments[1].weight).toBeGreaterThan(segments[0].weight);
  });

  test('a finished film marks every phase done', () => {
    const segments = buildStudioStageTimeline({
      ...base,
      status: 'succeeded',
      stage: 'render_done',
      hasStoryboard: true,
      hasFinalVideo: true,
      updatedAt: '2026-08-18T01:03:00.000Z',
      events: [
        { stage: 'planning', message: '', at: '2026-08-18T01:00:00.000Z' },
        { stage: 'render_start', message: '', at: '2026-08-18T01:01:00.000Z' },
        { stage: 'render_done', message: '', at: '2026-08-18T01:02:00.000Z' },
      ],
    });

    expect(segments.map((s) => s.state)).toEqual(['done', 'done', 'done', 'done']);
    expect(segments[3].durationMs).toBe(60_000);
  });

  test('re-entering planning after storyboard does not keep the brief phase live', () => {
    const segments = buildStudioStageTimeline({
      ...base,
      status: 'planning',
      stage: 'planning',
      hasStoryboard: true,
      nowMs: Date.parse('2026-08-18T01:10:00.000Z'),
      events: [
        { stage: 'develop_story', message: '', at: '2026-08-18T01:00:00.000Z' },
        { stage: 'design_storyboard', message: '', at: '2026-08-18T01:01:00.000Z' },
        { stage: 'planned', message: '', at: '2026-08-18T01:02:00.000Z' },
        { stage: 'cancelled', message: '', at: '2026-08-18T01:03:00.000Z' },
        { stage: 'planning', message: '', at: '2026-08-18T01:04:00.000Z' },
      ],
    });

    expect(segments.map((s) => s.state)).toEqual(['done', 'active', 'pending', 'pending']);
    expect(segments[0].live).toBe(false);
    expect(segments[1].live).toBe(true);
  });
});
