/**
 * Project ViMax run events + artifacts into a small set of session turns.
 *
 * Chapters follow a canonical narrative order, not the raw event log. Resume
 * and render-time recap stages (reuse_plan, extract_characters, portraits, …)
 * refresh media on an existing chapter instead of appending a second copy or
 * rewriting a finished title. Truncated logs are filled from artifacts so
 * earlier chapters do not vanish or appear after render.
 */

import { coalesceProgressEvents } from '../progressEventElapsed';
import type { StudioStageVariant } from '../studioStageTimeline';
import {
  collectFilmMedia,
  collectPortraitMedia,
  collectShotFrameMedia,
  collectShotVideoMedia,
  collectStoryDocuments,
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

const FILM_BEAT_ORDER: readonly StudioNarrativeBeat[] = [
  'plan',
  'portraits',
  'world',
  'storyboard',
  'render_frames',
  'render_clips',
  'film',
];

const ACTION_BEAT_ORDER: readonly StudioNarrativeBeat[] = [
  'action_assets',
  'action_generate',
  'film',
];

const RENDER_BEATS = new Set<StudioNarrativeBeat>([
  'render_frames',
  'render_clips',
  'film',
  'action_generate',
]);

const PLANNING_BEATS = new Set<StudioNarrativeBeat>([
  'plan',
  'portraits',
  'world',
  'storyboard',
  'action_assets',
]);

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
  voice_profiles: 'portraits',
  character_portraits_start: 'portraits',
  character_portraits_done: 'portraits',
  character_portrait_start: 'portraits',
  voice_references_start: 'portraits',
  voice_references_done: 'portraits',
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

function beatOrder(variant: StudioStageVariant): readonly StudioNarrativeBeat[] {
  return variant === 'action' ? ACTION_BEAT_ORDER : FILM_BEAT_ORDER;
}

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
    case 'plan':
      return collectStoryDocuments(input.artifacts);
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

interface BeatChapter {
  stage: string;
  at?: string;
}

function chapterAlreadySettled(
  beat: StudioNarrativeBeat,
  input: ProjectStudioSessionInput
): boolean {
  switch (beat) {
    case 'plan':
      return (
        Boolean(input.sourceText?.trim()) ||
        collectStoryDocuments(input.artifacts).length > 0 ||
        input.hasStoryboard
      );
    case 'portraits':
      return collectPortraitMedia(input.artifacts).length > 0;
    case 'world':
      return collectWorldMedia(input.artifacts).length > 0;
    case 'storyboard':
      return input.hasStoryboard;
    case 'action_assets':
      return Boolean(input.actionAssetsReady);
    default:
      return false;
  }
}

function isPlanningRecap(
  stage: string,
  beat: StudioNarrativeBeat,
  seenRender: boolean,
  rendering: boolean,
  input: ProjectStudioSessionInput
): boolean {
  if (stage === 'reuse_plan') return true;
  if (!PLANNING_BEATS.has(beat)) return false;
  if (seenRender) return true;
  if (!rendering) return false;
  return chapterAlreadySettled(beat, input);
}

/** Resume recap stages must not replace a finished planning title with a start. */
function planningStageRank(stage: string): number {
  if (
    stage.endsWith('_done') ||
    stage === 'planned' ||
    stage === 'develop_story' ||
    stage === 'write_script' ||
    stage === 'design_storyboard'
  ) {
    return 2;
  }
  if (stage.endsWith('_start') || stage === 'planning' || stage === 'extract_characters') {
    return 0;
  }
  return 1;
}

function settledStageForBeat(
  beat: StudioNarrativeBeat,
  stage: string,
  input: ProjectStudioSessionInput
): string {
  switch (beat) {
    case 'plan': {
      const docs = collectStoryDocuments(input.artifacts);
      if (docs.some((doc) => doc.role === 'story')) return 'develop_story';
      if (docs.some((doc) => doc.role === 'script')) return 'write_script';
      if (docs.some((doc) => doc.role === 'cast')) return 'extract_characters';
      return stage === 'reuse_plan' ? 'extract_characters' : stage;
    }
    case 'portraits':
      return collectPortraitMedia(input.artifacts).length > 0
        ? 'character_portraits_done'
        : stage;
    case 'world':
      return collectWorldMedia(input.artifacts).length > 0 ? 'world_assets_done' : stage;
    case 'storyboard':
      return input.hasStoryboard ? 'planned' : stage;
    case 'render_frames':
      return collectShotFrameMedia(input.artifacts).length > 0 ? 'frames_done' : stage;
    case 'render_clips':
      return collectShotVideoMedia(input.artifacts).length > 0 ? 'video_clips_done' : stage;
    case 'film':
      return input.hasFinalVideo ? 'render_done' : stage;
    case 'action_assets':
      return input.actionAssetsReady ? 'planned' : stage;
    default:
      return stage;
  }
}

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
  const docs = collectStoryDocuments(input.artifacts);
  const hasPlanTrail =
    Boolean(input.sourceText?.trim()) ||
    input.hasStoryboard ||
    portraits.length > 0 ||
    world.length > 0 ||
    docs.length > 0;

  if (hasPlanTrail) {
    if (docs.some((doc) => doc.role === 'story')) push('develop_story');
    else if (docs.some((doc) => doc.role === 'script')) push('write_script');
    else push('extract_characters');
  }
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

function seedMissingChapters(
  chapters: Map<StudioNarrativeBeat, BeatChapter>,
  input: ProjectStudioSessionInput,
  variant: StudioStageVariant
): void {
  for (const event of synthesizeStudioHistoryEvents(input)) {
    const beat = beatOf(event.stage, variant);
    if (!beat || chapters.has(beat)) continue;
    chapters.set(beat, { stage: event.stage, at: event.at });
  }
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
  const rendering = runStatus === 'rendering';

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
  const chapters = new Map<StudioNarrativeBeat, BeatChapter>();
  let seenRender = false;

  for (const { event } of coalesced) {
    const beat = beatOf(event.stage, variant);
    if (!beat) continue;
    const recap = isPlanningRecap(event.stage, beat, seenRender, rendering, input);
    if (recap) {
      if (!chapters.has(beat)) {
        chapters.set(beat, {
          stage: settledStageForBeat(beat, event.stage, input),
          at: event.at,
        });
      }
      continue;
    }
    const existing = chapters.get(beat);
    if (
      existing &&
      PLANNING_BEATS.has(beat) &&
      planningStageRank(event.stage) < planningStageRank(existing.stage)
    ) {
      continue;
    }
    chapters.set(beat, { stage: event.stage, at: event.at });
    if (RENDER_BEATS.has(beat)) seenRender = true;
  }

  seedMissingChapters(chapters, input, variant);

  if (
    busy &&
    runStatus === 'rendering' &&
    variant !== 'action' &&
    !chapters.has('render_frames') &&
    !chapters.has('render_clips') &&
    !chapters.has('film')
  ) {
    chapters.set('render_frames', {
      stage: 'render_start',
      at: status?.updated_at ?? undefined,
    });
  }

  const statusStage = status?.stage;
  const statusBeat = beatOf(statusStage, variant);
  const statusIsRecap =
    statusStage && statusBeat
      ? isPlanningRecap(statusStage, statusBeat, seenRender, rendering, input)
      : false;

  let liveBeat: StudioNarrativeBeat | null = null;
  if (busy) {
    if (statusBeat && !statusIsRecap) {
      liveBeat = statusBeat;
    } else {
      for (const beat of [...beatOrder(variant)].reverse()) {
        if (chapters.has(beat) && RENDER_BEATS.has(beat)) {
          liveBeat = beat;
          break;
        }
      }
      if (!liveBeat) {
        for (const beat of [...beatOrder(variant)].reverse()) {
          if (chapters.has(beat)) {
            liveBeat = beat;
            break;
          }
        }
      }
    }
  }

  for (const beat of beatOrder(variant)) {
    const chapter = chapters.get(beat);
    if (!chapter) continue;
    messages.push({
      id: `beat:${beat}`,
      role: 'assistant',
      kind: 'milestone',
      beat,
      stage: chapter.stage,
      at: chapter.at,
      live: liveBeat === beat,
      pollWaitSecs: beat === 'render_clips' || beat === 'action_generate' ? pollWaitSecs : null,
      media: mediaForBeat(beat, input),
    });
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
    const film = messages.find((item) => item.beat === 'film');
    if (film) {
      film.kind = 'film_ready';
      film.live = false;
      film.media = collectFilmMedia(input.finalVideoPath, input.coverPath);
    } else {
      messages.push({
        id: 'film-ready',
        role: 'assistant',
        kind: 'film_ready',
        beat: 'film',
        media: collectFilmMedia(input.finalVideoPath, input.coverPath),
      });
    }
  }

  return messages;
}
