/**
 * Aggregate fine-grained pipeline progress events into the macro studio phases
 * (Creative / Storyboard / Render / Film, or Assets / Generate / Film for
 * action imitation) with per-phase wall-clock duration, so the stage rail can
 * size each segment by the time it took.
 */

import { parseEventMs } from './progressEventElapsed';
import type { SessionStatus, VimaxRunStatus } from './types';

export type StudioStageVariant = 'film' | 'action';

export type StudioStageKey =
  | 'brief'
  | 'storyboard'
  | 'render'
  | 'film'
  | 'assets'
  | 'generate';

export type StudioStageState = 'pending' | 'active' | 'done' | 'failed' | 'cancelled';

const STAGE_KEYS_BY_VARIANT: Record<StudioStageVariant, readonly StudioStageKey[]> = {
  film: ['brief', 'storyboard', 'render', 'film'],
  action: ['assets', 'generate', 'film'],
};

export function studioStageKeys(
  variant: StudioStageVariant = 'film'
): readonly StudioStageKey[] {
  return STAGE_KEYS_BY_VARIANT[variant];
}

export interface StudioStageSegment {
  key: StudioStageKey;
  state: StudioStageState;
  /** Accumulated time spent in this phase (ms); null when never entered. */
  durationMs: number | null;
  /** Epoch ms of the first event that entered this phase. */
  startedAtMs: number | null;
  /** The phase clock is still running. */
  live: boolean;
  /** `flex-grow` weight; equal for every phase before any timing exists. */
  weight: number;
}

export interface StudioStageTimelineInput {
  status?: VimaxRunStatus | null;
  stage?: string | null;
  events?: SessionStatus['events'];
  updatedAt?: string | null;
  nowMs: number;
  hasStoryboard: boolean;
  hasFinalVideo: boolean;
  /** `action` = action-imitation runs, which have no planning or storyboard. */
  variant?: StudioStageVariant;
}

/** Text planning: idea/script/novel understanding, characters, script. */
const BRIEF_STAGES = new Set([
  'planning',
  'save_novel',
  'compress_novel',
  'compress_aggregate',
  'extract_events',
  'event_rag',
  'extract_scenes',
  'merge_characters',
  'plan_scene',
  'develop_story',
  'extract_characters',
  'write_script',
  'plan',
]);

/** Shot design plus the reference assets produced while planning. */
const STORYBOARD_STAGES = new Set([
  'design_storyboard',
  'decompose_shots',
  'construct_camera_tree',
  'planned',
  'reuse_plan',
  'character_portraits_start',
  'character_portraits_done',
  'character_portrait_start',
  'world_assets_start',
  'world_assets_done',
  'revise',
]);

/** Keyframes, clips, and everything billed as rendering. */
const RENDER_STAGES = new Set([
  'rendering',
  'render',
  'render_start',
  'render_scene',
  'render_scene_skip',
  'render_scene_done',
  'render_scene_failed',
  'render_resume',
  'frames_start',
  'frames_done',
  'frames_cancelled',
  'frame_camera_start',
  'frame_camera_done',
  'frame_start',
  'frame_prompt_start',
  'frame_done',
  'image_generate',
  'film_cover_start',
  'film_cover_done',
  'video_clips_start',
  'video_clip_exists',
  'video_clip_start',
  'video_clip_done',
  'video_clips_partial',
  'video_clips_done',
  'video_create',
  'video_poll',
  'video_download',
  'video_generate',
  'video_continuity',
  'concat_start',
]);

/** Muxing done — the deliverable exists. */
const FILM_STAGES = new Set(['concat_done', 'render_done', 'final_video_exists']);

/** Action imitation: validating the character still and the reference video. */
const ACTION_ASSET_STAGES = new Set(['action_prepare', 'planned']);

/** Action imitation: the single clip plus its cover. */
const ACTION_GENERATE_STAGES = new Set([
  'action_generate',
  'rendering',
  'render',
  'render_start',
  'film_cover_start',
  'film_cover_done',
  'video_create',
  'video_poll',
  'video_download',
  'video_generate',
]);

/** Macro phase for a backend stage key; null for terminal markers. */
export function macroStageOf(
  stage: string | null | undefined,
  variant: StudioStageVariant = 'film'
): StudioStageKey | null {
  if (!stage) return null;
  if (FILM_STAGES.has(stage)) return 'film';
  if (variant === 'action') {
    if (ACTION_ASSET_STAGES.has(stage)) return 'assets';
    if (ACTION_GENERATE_STAGES.has(stage)) return 'generate';
    return null;
  }
  if (BRIEF_STAGES.has(stage)) return 'brief';
  if (STORYBOARD_STAGES.has(stage)) return 'storyboard';
  if (RENDER_STAGES.has(stage)) return 'render';
  return null;
}

function stageIndex(key: StudioStageKey, variant: StudioStageVariant): number {
  return studioStageKeys(variant).indexOf(key);
}

/** Each segment keeps this share of the bar so short phases stay readable. */
const MIN_WEIGHT_SHARE = 0.1;

function assignWeights(durations: Array<number | null>): number[] {
  const raw = durations.map((d) => (d != null && d > 0 ? d : 0));
  const total = raw.reduce((sum, value) => sum + value, 0);
  if (total <= 0) return raw.map(() => 1);
  const floor = total * MIN_WEIGHT_SHARE;
  return raw.map((value) => Math.max(value, floor));
}

/**
 * Highest phase reached, from live status plus the artifacts already on disk.
 * `-1` means the run has not started yet.
 */
export function studioStageActiveIndex(input: StudioStageTimelineInput): number {
  const { status, stage, hasStoryboard, hasFinalVideo } = input;
  const variant = input.variant ?? 'film';
  const at = (key: StudioStageKey) => stageIndex(key, variant);
  if (hasFinalVideo) return at('film');

  let index = -1;
  for (const ev of input.events ?? []) {
    const key = macroStageOf(ev.stage, variant);
    if (key) index = Math.max(index, at(key));
  }

  const fromStage = macroStageOf(stage, variant);
  if (fromStage) index = Math.max(index, at(fromStage));

  if (status === 'succeeded') index = Math.max(index, at('film'));
  if (variant === 'action') {
    if (status === 'rendering') index = Math.max(index, at('generate'));
    if (status === 'planning') index = Math.max(index, at('assets'));
    return index;
  }

  if (status === 'rendering') index = Math.max(index, at('render'));
  if (status === 'planning') index = Math.max(index, at('brief'));
  if (hasStoryboard) index = Math.max(index, at('storyboard'));

  return index;
}

/** Phase that owns a failure: the last phase the pipeline was actually in. */
function failureIndex(input: StudioStageTimelineInput, fallback: number): number {
  const variant = input.variant ?? 'film';
  const events = [...(input.events ?? [])].reverse();
  for (const ev of events) {
    const key = macroStageOf(ev.stage, variant);
    if (key) return stageIndex(key, variant);
  }
  const fromStage = macroStageOf(input.stage, variant);
  return fromStage ? stageIndex(fromStage, variant) : fallback;
}

export function buildStudioStageTimeline(
  input: StudioStageTimelineInput
): StudioStageSegment[] {
  const events = input.events ?? [];
  const variant = input.variant ?? 'film';
  const keys = studioStageKeys(variant);
  const busy = input.status === 'planning' || input.status === 'rendering';

  const durations: Array<number | null> = keys.map(() => null);
  const startedAt: Array<number | null> = keys.map(() => null);
  let liveIndex = -1;

  for (let i = 0; i < events.length; i++) {
    const key = macroStageOf(events[i].stage, variant);
    if (!key) continue;
    const start = parseEventMs(events[i].at);
    if (start == null) continue;
    const index = stageIndex(key, variant);

    const current = startedAt[index];
    startedAt[index] = current == null ? start : Math.min(current, start);

    let end: number | null = null;
    for (let j = i + 1; j < events.length; j++) {
      end = parseEventMs(events[j].at);
      if (end != null) break;
    }
    if (end == null) {
      if (busy) {
        end = input.nowMs;
        liveIndex = index;
      } else {
        end = parseEventMs(input.updatedAt ?? undefined);
      }
    }
    if (end == null) continue;

    durations[index] = (durations[index] ?? 0) + Math.max(0, end - start);
  }

  const activeIndex = studioStageActiveIndex(input);
  const failed = input.status === 'failed';
  const cancelled = input.status === 'cancelled';
  const terminalIndex =
    failed || cancelled ? failureIndex(input, Math.max(0, activeIndex)) : -1;
  const weights = assignWeights(durations);

  return keys.map((key, index) => {
    let state: StudioStageState;
    if (failed || cancelled) {
      state =
        index < terminalIndex
          ? 'done'
          : index === terminalIndex
            ? failed
              ? 'failed'
              : 'cancelled'
            : 'pending';
    } else if (input.hasFinalVideo || input.status === 'succeeded') {
      state = 'done';
    } else if (activeIndex < 0 || index > activeIndex) {
      state = 'pending';
    } else if (index < activeIndex) {
      state = 'done';
    } else {
      state = busy ? 'active' : 'done';
    }

    return {
      key,
      state,
      durationMs: durations[index],
      startedAtMs: startedAt[index],
      live: state === 'active' && (liveIndex === index || durations[index] == null),
      weight: weights[index],
    };
  });
}
