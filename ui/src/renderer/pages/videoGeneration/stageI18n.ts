/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { TFunction } from 'i18next';

/** Stages that have entries under `videoGeneration.stages.*`. */
const KNOWN_STAGES = new Set([
  'planning',
  'rendering',
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
  'render_start',
  'render_scene',
  'render_scene_skip',
  'render_scene_done',
  'render_scene_failed',
  'render_resume',
  'frames_start',
  'frame_camera_start',
  'frame_camera_done',
  'frames_done',
  'frames_cancelled',
  'frame_start',
  'frame_prompt_start',
  'frame_done',
  'video_clips_start',
  'video_clip_exists',
  'video_clip_start',
  'video_clip_done',
  'video_clips_partial',
  'video_clips_done',
  'video_generate',
  'concat_start',
  'concat_done',
  'render_done',
  'final_video_exists',
  'image_generate',
  'failed',
  'cancelled',
  'plan',
  'render',
  'revise',
]);

/** True when the stage has a dedicated i18n label (prefer over backend message). */
export function isKnownStage(stage: string | null | undefined): boolean {
  return !!stage && KNOWN_STAGES.has(stage);
}

/** Human-readable labels for pipeline stage keys (i18n). Never returns backend Chinese messages. */
export function stageLabel(stage: string | null | undefined, t: TFunction): string {
  if (!stage) return '';
  const key = `videoGeneration.stages.${stage}`;
  const translated = t(key, { defaultValue: '' });
  if (translated) return translated;
  // Last resort: show the machine stage key (English snake_case), not a localized backend string.
  return stage;
}

/**
 * Status line under the workspace title / in progress cards.
 * Prefer translated stage; never surface raw backend Chinese progress messages.
 */
export function progressStatusText(
  stage: string | null | undefined,
  message: string | null | undefined,
  t: TFunction
): string {
  const label = stageLabel(stage, t);
  if (label && label !== stage) return label;
  if (isKnownStage(stage)) return label;
  // Unknown stage: still prefer stage key over Chinese message.
  if (stage) return stage;
  const msg = message?.trim() || '';
  // Drop CJK-only backend leftovers when UI language expects i18n elsewhere.
  if (msg && /[\u4e00-\u9fff]/.test(msg)) return '';
  return msg;
}
