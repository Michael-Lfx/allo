/**
 * Project ViMax run events + artifacts into a small set of session turns.
 *
 * Each narrative beat owns at most one bubble. Later heartbeats and render-time
 * recap stages (reuse_plan, character_portraits_start, …) update that bubble
 * instead of appending a second copy. Title/stage only advances while the beat
 * is still the contiguous frontier — recaps must not rewrite a finished chapter.
 */

import { coalesceProgressEvents } from '../progressEventElapsed';
import type { StudioStageVariant } from '../studioStageTimeline';
import {
  collectFilmMedia,
  collectPortraitMedia,
  collectShotFrameMedia,
  collectShotVideoMedia,
  collectWorldMedia,
} from './collectStudioMedia';
import type {
  ProjectStudioSessionInput,
  StudioComposerAction,
  StudioComposerActionInput,
  StudioNarrativeBeat,
  StudioSessionMedia,
  StudioSessionMessage,
} from './types';
import type { SessionStatus } from '../types';

const TERMINAL_STAGES = new Set(['failed', 'cancelled', 'interrupted']);

const FILM_BEAT_STAGES: Record<string, StudioNarrativeBeat> = {
  planning: 'plan',
  save_novel: 'plan',
  compress_novel: 'plan',
  compress_aggregate: 'plan',
  extract_events: 'plan',
  event_rag: 'plan',
  extract_scenes: 'plan',
  merge_characters: 'plan',
  develop_story: 'plan',
  extract_characters: 'plan',
  write_script: 'plan',
  plan: 'plan',
  plan_scene: 'storyboard',
  design_storyboard: 'storyboard',
  decompose_shots: 'storyboard',
  construct_camera_tree: 'storyboard',
  planned: 'storyboard',
  reuse_plan: 'storyboard',
  revise: 'storyboard',
  character_portraits_start: 'portraits',
  character_portraits_done: 'portraits',
  character_portrait_start: 'portraits',
  world_assets_start: 'world',
  world_assets_done: 'world',
  rendering: 'render_frames',
  render: 'render_frames',
  render_start: 'render_frames',
  render_scene: 'render_frames',
  render_scene_skip: 'render_frames',
  render_scene_done: 'render_frames',
  render_resume: 'render_frames',
  frames_start: 'render_frames',
  frames_done: 'render_frames',
  frames_cancelled: 'render_frames',
  frame_camera_start: 'render_frames',
  frame_camera_done: 'render_frames',
  frame_start: 'render_frames',
  frame_prompt_start: 'render_frames',
  frame_done: 'render_frames',
  image_generate: 'render_frames',
  film_cover_start: 'film',
  film_cover_done: 'film',
  video_clips_start: 'render_clips',
  video_clip_exists: 'render_clips',
  video_clip_start: 'render_clips',
  video_clip_done: 'render_clips',
  video_clips_partial: 'render_clips',
  video_clips_done: 'render_clips',
  video_create: 'render_clips',
  video_poll: 'render_clips',
  video_download: 'render_clips',
  video_generate: 'render_clips',
  video_continuity: 'render_clips',
  concat_start: 'film',
  concat_done: 'film',
  render_done: 'film',
  final_video_exists: 'film',
};

const ACTION_BEAT_STAGES: Record<string, StudioNarrativeBeat> = {
  action_prepare: 'action_assets',
  planned: 'action_assets',
  action_generate: 'action_generate',
  rendering: 'action_generate',
  render: 'action_generate',
  render_start: 'action_generate',
  film_cover_start: 'film',
  film_cover_done: 'film',
  video_create: 'action_generate',
  video_poll: 'action_generate',
  video_download: 'action_generate',
  video_generate: 'action_generate',
  concat_done: 'film',
  render_done: 'film',
  final_video_exists: 'film',
};

function beatOf(
  stage: string | undefined,
  variant: StudioStageVariant
): StudioNarrativeBeat | null {
  if (!stage || TERMINAL_STAGES.has(stage)) return null;
  const table = variant === 'action' ? ACTION_BEAT_STAGES : FILM_BEAT_STAGES;
  return table[stage] ?? null;
}

function latestPollWait(events: NonNullable<ProjectStudioSessionInput['status']>['events']): number | null {
  const list = events ?? [];
  for (let i = list.length - 1; i >= 0; i--) {
    if (list[i].stage !== 'video_poll') continue;
    const meta = list[i].metadata as { elapsed_secs?: number } | null | undefined;
    if (typeof meta?.elapsed_secs === 'number') return meta.elapsed_secs;
  }
  return null;
}

function mediaForBeat(
  beat: StudioNarrativeBeat,
  input: ProjectStudioSessionInput
): StudioSessionMedia[] {
  switch (beat) {
    case 'portraits':
      return collectPortraitMedia(input.artifacts);
    case 'world':
      return collectWorldMedia(input.artifacts);
    case 'render_frames':
      return collectShotFrameMedia(input.artifacts);
    case 'render_clips':
    case 'action_generate': {
      const clips = collectShotVideoMedia(input.artifacts);
      return clips.length > 0 ? clips : collectShotFrameMedia(input.artifacts);
    }
    case 'film':
      return collectFilmMedia(input.finalVideoPath, input.coverPath);
    default:
      return [];
  }
}

type StatusEvent = NonNullable<SessionStatus['events']>[number];

/** Rebuild a chapter list from artifacts when the process restarted without a log. */
export function synthesizeStudioHistoryEvents(
  input: ProjectStudioSessionInput
): StatusEvent[] {
  const at = input.status?.updated_at ?? '';
  const events: StatusEvent[] = [];
  const push = (stage: string) => {
    events.push({ stage, message: '', at });
  };

  if (input.isAction) {
    if (input.actionAssetsReady || input.hasFinalVideo) push('action_prepare');
    if (collectShotVideoMedia(input.artifacts).length > 0 || input.hasFinalVideo) {
      push('action_generate');
    }
    if (input.hasFinalVideo) push('render_done');
    return events;
  }

  const portraits = collectPortraitMedia(input.artifacts);
  const world = collectWorldMedia(input.artifacts);
  const frames = collectShotFrameMedia(input.artifacts);
  const clips = collectShotVideoMedia(input.artifacts);
  const hasPlanTrail =
    Boolean(input.sourceText?.trim()) ||
    input.hasStoryboard ||
    portraits.length > 0 ||
    world.length > 0;

  if (hasPlanTrail) push('extract_characters');
  if (portraits.length > 0) push('character_portraits_done');
  if (world.length > 0) push('world_assets_done');
  if (input.hasStoryboard) push('planned');
  if (frames.length > 0) push('frames_done');
  if (clips.length > 0) push('video_clips_done');
  if (input.hasFinalVideo) push('render_done');
  return events;
}

function eventsForProjection(input: ProjectStudioSessionInput): StatusEvent[] {
  const recorded = input.status?.events ?? [];
  if (recorded.length > 0) return recorded;
  return synthesizeStudioHistoryEvents(input);
}

export function resolveStudioComposerAction(
  input: StudioComposerActionInput
): StudioComposerAction {
  if (input.busy) return 'stop';
  if (input.hasFinalVideo) return 'none';
  if (input.isFailed) return 'continue';
  if (input.isAction) {
    if (input.canRender && input.actionAssetsReady) return 'render';
    return 'none';
  }
  if (input.hasStoryboard) return 'render';
  return 'plan';
}

export function projectStudioSessionMessages(
  input: ProjectStudioSessionInput
): StudioSessionMessage[] {
  const messages: StudioSessionMessage[] = [];
  const variant = input.variant ?? (input.isAction ? 'action' : 'film');
  const status = input.status ?? null;
  const events = eventsForProjection(input);
  const runStatus = input.runStatus ?? status?.status ?? null;
  const busy = runStatus === 'planning' || runStatus === 'rendering';

  const brief = input.sourceText?.trim() ?? '';
  const briefMedia = input.briefMedia ?? [];
  if (brief || briefMedia.length > 0) {
    messages.push({
      id: 'user-brief',
      role: 'user',
      kind: 'user_brief',
      text: brief,
      media: briefMedia.length > 0 ? briefMedia : undefined,
    });
  }

  for (const note of input.notes ?? []) {
    if (!note.text.trim()) continue;
    messages.push({
      id: note.id,
      role: 'user',
      kind: 'user_note',
      text: note.text.trim(),
    });
  }

  const coalesced = coalesceProgressEvents(events);
  const pollWaitSecs = latestPollWait(events);
  const beatIndex = new Map<StudioNarrativeBeat, number>();
  let frontierBeat: StudioNarrativeBeat | null = null;

  for (const { event } of coalesced) {
    const beat = beatOf(event.stage, variant);
    if (!beat) continue;

    const existingAt = beatIndex.get(beat);
    if (existingAt !== undefined) {
      const last = messages[existingAt];
      if (last && last.kind === 'milestone') {
        last.at = event.at;
        last.pollWaitSecs = event.stage === 'video_poll' ? pollWaitSecs : last.pollWaitSecs;
        last.media = mediaForBeat(beat, input);
        // Contiguous work on the same beat (poll ticks, portraits_start→done)
        // may refresh the title. Render recaps (reuse_plan, or a beat that
        // already finished and then reappears) must not rewrite the chapter.
        if (frontierBeat === beat && event.stage !== 'reuse_plan') {
          last.stage = event.stage;
        }
      }
      frontierBeat = beat;
      continue;
    }

    beatIndex.set(beat, messages.length);
    frontierBeat = beat;
    messages.push({
      id: `beat:${beat}`,
      role: 'assistant',
      kind: 'milestone',
      beat,
      stage: event.stage,
      at: event.at,
      live: false,
      pollWaitSecs: event.stage === 'video_poll' ? pollWaitSecs : null,
      media: mediaForBeat(beat, input),
    });
  }

  const currentBeat = beatOf(status?.stage, variant);
  for (const msg of messages) {
    if (msg.kind !== 'milestone') continue;
    msg.live = busy && currentBeat !== null && msg.beat === currentBeat;
  }

  const failedLike =
    runStatus === 'failed' || runStatus === 'cancelled' || runStatus === 'interrupted';

  if (runStatus === 'failed' && status?.error) {
    messages.push({
      id: 'failure',
      role: 'error',
      kind: 'failure',
      stage: status.stage,
      error: status.error,
      at: status.updated_at ?? undefined,
    });
  } else if (runStatus === 'cancelled' || runStatus === 'interrupted') {
    messages.push({
      id: `terminal:${runStatus}`,
      role: 'system',
      kind: 'cancelled',
      stage: runStatus,
      at: status?.updated_at ?? undefined,
    });
  }

  const awaitingRender =
    !busy &&
    !failedLike &&
    !input.hasFinalVideo &&
    (input.isAction ? Boolean(input.actionAssetsReady) : input.hasStoryboard);

  if (awaitingRender) {
    messages.push({
      id: input.isAction ? 'gate-action' : 'gate-render',
      role: 'assistant',
      kind: input.isAction ? 'gate_action' : 'gate_render',
    });
  }

  if (input.hasFinalVideo && !failedLike) {
    const hasFilmMsg = messages.some((m) => m.kind === 'film_ready' || m.beat === 'film');
    if (!hasFilmMsg) {
      messages.push({
        id: 'film-ready',
        role: 'assistant',
        kind: 'film_ready',
        beat: 'film',
        media: collectFilmMedia(input.finalVideoPath, input.coverPath),
      });
    } else {
      const film = [...messages].reverse().find((m) => m.beat === 'film');
      if (film) {
        film.kind = 'film_ready';
        film.live = false;
        film.media = collectFilmMedia(input.finalVideoPath, input.coverPath);
      }
    }
  }

  return messages;
}
