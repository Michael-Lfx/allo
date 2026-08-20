import type { SessionStatus } from './types';

const TERMINAL_EVENT_STAGES = new Set([
  'failed',
  'cancelled',
  'interrupted',
  'planning',
  'rendering',
]);

/** Stages that mean the creative/plan phase already finished — resume should render. */
const PLAN_COMPLETE_STAGES = new Set(['planned', 'reuse_plan']);

const RENDER_STAGES = new Set([
  'render_start',
  'rendering',
  'render_scene',
  'render_resume',
  'render_scene_skip',
  'reuse_plan',
  'character_portraits_start',
  'frames_start',
  'frame_prompt_start',
  'video_clips_start',
  'concat_start',
  'video_generate',
  'image_generate',
  'action_prepare',
  'action_generate',
  'video_poll',
  'video_create',
  'video_download',
  'video_clip_start',
  'video_clip_done',
  'frames_done',
]);

/**
 * Decide whether "continue from checkpoint" should resume rendering or planning.
 *
 * Terminal events (`cancelled` / `interrupted` / `failed`) and status heartbeats
 * (`planning` / `rendering`) must not hide the last real pipeline stage.
 */
export function shouldContinueAsRender(options: {
  events?: SessionStatus['events'];
  stage?: string | null;
  sessionStage?: string | null;
}): boolean {
  const events = options.events ?? [];
  const beforeTerminal = [...events]
    .reverse()
    .find((event) => event.stage && !TERMINAL_EVENT_STAGES.has(event.stage));

  const stage = beforeTerminal?.stage || options.stage || options.sessionStage || '';
  if (!stage || TERMINAL_EVENT_STAGES.has(stage)) {
    return events.some((event) => PLAN_COMPLETE_STAGES.has(event.stage) || isRenderStage(event.stage));
  }
  if (PLAN_COMPLETE_STAGES.has(stage)) return true;
  return isRenderStage(stage);
}

function isRenderStage(stage: string): boolean {
  return RENDER_STAGES.has(stage) || stage.startsWith('render_') || stage.startsWith('video_');
}
