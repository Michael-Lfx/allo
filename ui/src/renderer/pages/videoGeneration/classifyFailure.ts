/**
 * Classify a ViMax run failure for user-facing copy.
 * Shared by the agent session panel (and the unused ProgressTimeline).
 */

import type { TFunction } from 'i18next';
import { isInsufficientCreditsError } from './creditsError';
import type { SessionStatus } from './types';

export type FailureKind = 'credits' | 'llm' | 'image' | 'video' | 'unknown';

export interface ClassifiedFailure {
  kind: FailureKind;
  title: string;
  hint: string;
}

const PLANNING_LLM_STAGES = new Set([
  'planning',
  'develop_story',
  'extract_characters',
  'write_script',
  'plan_scene',
  'design_storyboard',
  'decompose_shots',
  'construct_camera_tree',
  'compress_novel',
  'compress_aggregate',
  'extract_events',
  'event_rag',
  'extract_scenes',
  'merge_characters',
]);

const RENDER_LLM_STAGES = new Set([
  'reuse_plan',
  'render_start',
  'rendering',
  'render_scene',
  'render_resume',
  'frame_prompt_start',
]);

const IMAGE_STAGES = new Set([
  'character_portraits_start',
  'world_assets_start',
  'frames_start',
  'frame_camera_start',
  'frame_start',
  'frame_prompt_start',
  'image_generate',
]);

const VIDEO_STAGES = new Set([
  'video_clips_start',
  'video_create',
  'video_poll',
  'video_download',
  'video_generate',
  'concat_start',
]);

export function classifyFailure(
  error: string,
  stage: string | null | undefined,
  events: SessionStatus['events'],
  t: TFunction
): ClassifiedFailure {
  const lower = error.toLowerCase();
  const isChannel = lower.includes('all channel models failed');

  if (isInsufficientCreditsError(error)) {
    return {
      kind: 'credits',
      title: t('videoGeneration.workspace.failure.creditsTitle', {
        defaultValue: '积分不足',
      }),
      hint: t('videoGeneration.workspace.failure.creditsHint', {
        defaultValue:
          '当前积分不足以完成本次生成。请充值或缩短时长后，点击「从断点继续」；已成功的片段不会重复扣费。',
      }),
    };
  }

  const beforeFail = [...(events ?? [])]
    .reverse()
    .find(
      (e) =>
        e.stage && e.stage !== 'failed' && e.stage !== 'cancelled' && e.stage !== 'interrupted'
    );
  const stageKey = beforeFail?.stage || stage || '';

  let looksLikeLlm =
    lower.includes('llm failed') ||
    lower.includes('规划模型') ||
    lower.includes('聊天模型') ||
    lower.includes('chat_completions') ||
    lower.includes('empty content');

  const looksLikeBadImage =
    lower.includes('invalid png') ||
    lower.includes('open ref') ||
    lower.includes('decode image') ||
    lower.includes('downloaded image is not') ||
    (lower.includes('media processing') &&
      (lower.includes('.png') ||
        lower.includes('three_view') ||
        lower.includes('character_portrait')));

  let kind: FailureKind = 'unknown';
  if (looksLikeBadImage) {
    kind = 'image';
  } else if (looksLikeLlm || PLANNING_LLM_STAGES.has(stageKey) || RENDER_LLM_STAGES.has(stageKey)) {
    kind = 'llm';
  } else if (lower.includes('image') || lower.includes('图片') || IMAGE_STAGES.has(stageKey)) {
    kind = 'image';
  } else if (
    lower.includes('video generation failed') ||
    lower.includes('视频生成') ||
    VIDEO_STAGES.has(stageKey)
  ) {
    kind = 'video';
  } else if (isChannel) {
    kind = 'llm';
  }

  const inRenderPhase = RENDER_LLM_STAGES.has(stageKey) || stageKey.startsWith('render_');

  if (kind === 'llm') {
    const isCameraTree = lower.includes('camera tree length mismatch');
    return {
      kind,
      title: inRenderPhase
        ? t('videoGeneration.workspace.failure.llmRenderTitle')
        : isCameraTree
          ? t('videoGeneration.workspace.failure.llmCameraTreeTitle')
          : t('videoGeneration.workspace.failure.llmPlanTitle'),
      hint: isCameraTree
        ? t('videoGeneration.workspace.failure.llmCameraTreeHint')
        : isChannel
          ? inRenderPhase
            ? t('videoGeneration.workspace.failure.llmChannelRenderHint')
            : t('videoGeneration.workspace.failure.llmChannelPlanHint')
          : inRenderPhase
            ? t('videoGeneration.workspace.failure.llmRenderHint')
            : t('videoGeneration.workspace.failure.llmPlanHint'),
    };
  }
  if (kind === 'image') {
    return {
      kind,
      title: t('videoGeneration.workspace.failure.imageTitle'),
      hint: isChannel
        ? t('videoGeneration.workspace.failure.imageChannelHint')
        : t('videoGeneration.workspace.failure.imageHint'),
    };
  }
  if (kind === 'video') {
    return {
      kind,
      title: t('videoGeneration.workspace.failure.videoTitle'),
      hint: isChannel
        ? t('videoGeneration.workspace.failure.videoChannelHint')
        : t('videoGeneration.workspace.failure.videoHint'),
    };
  }
  return {
    kind,
    title: t('videoGeneration.workspace.failure.unknownTitle'),
    hint: t('videoGeneration.workspace.failure.unknownHint'),
  };
}
