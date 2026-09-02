/**
 * Classify a ViMax run failure for user-facing copy.
 * Shared by the agent session panel (and the unused ProgressTimeline).
 */

import type { TFunction } from 'i18next';
import { isInsufficientCreditsError } from './creditsError';
import {
  isContentPolicyRejection,
  isCopyrightRestriction,
  isReferenceImageModeration,
  extractProviderErrorCode,
  extractProviderErrorMessage,
} from './providerError';
import type { SessionStatus } from './types';

export type FailureKind = 'credits' | 'llm' | 'image' | 'video' | 'moderation' | 'unknown';

export interface ClassifiedFailure {
  kind: FailureKind;
  title: string;
  hint: string;
  errorCode?: string;
  providerMessage?: string;
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

  const providerMessage = extractProviderErrorMessage(error) ?? undefined;
  const errorCode = extractProviderErrorCode(error) ?? undefined;

  if (isContentPolicyRejection(error)) {
    if (isCopyrightRestriction(error)) {
      return {
        kind: 'moderation',
        title: t('videoGeneration.workspace.failure.copyrightTitle', {
          defaultValue: '成片未通过版权审核',
        }),
        hint: t('videoGeneration.workspace.failure.copyrightHint', {
          defaultValue:
            '生成画面可能涉及版权受限内容，该镜头未生成成功。请修改分镜描述或参考素材后，点击「从断点继续」；已成功的片段不会重复扣费。',
        }),
        errorCode,
        providerMessage,
      };
    }
    if (isReferenceImageModeration(error)) {
      return {
        kind: 'moderation',
        title: t('videoGeneration.workspace.failure.privacyTitle', {
          defaultValue: '参考图未通过内容审核',
        }),
        hint: t('videoGeneration.workspace.failure.privacyHint', {
          defaultValue:
            '参考图或首帧可能含真人肖像。请更换参考图或改用更偏插画的风格后，点击「从断点继续」。',
        }),
        errorCode,
        providerMessage,
      };
    }
    return {
      kind: 'moderation',
      title: t('videoGeneration.workspace.failure.moderationTitle', {
        defaultValue: '成片未通过内容审核',
      }),
      hint: t('videoGeneration.workspace.failure.moderationHint', {
        defaultValue:
          '模型判定本次生成内容不符合安全规范。请修改分镜描述或参考素材后，点击「从断点继续」；已成功的片段不会重复扣费。',
      }),
      errorCode,
      providerMessage,
    };
  }

  const looksLikeJsonArtifact =
    /empty json artifact/i.test(error) ||
    /unreadable json artifact/i.test(error) ||
    /json error at /i.test(error) ||
    /interrupted write or concurrent planner/i.test(error) ||
    (/^\s*json error:/i.test(error) && !/failed to parse llm json/i.test(error));

  if (looksLikeJsonArtifact) {
    return {
      kind: 'unknown',
      title: t('videoGeneration.workspace.failure.jsonArtifactTitle', {
        defaultValue: '规划产物读写失败',
      }),
      hint: t('videoGeneration.workspace.failure.jsonArtifactHint', {
        defaultValue:
          '场景规划时读到了不完整的 JSON 文件，通常不是规划模型本身报错。请点击「从断点继续」重试规划。',
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
      errorCode,
      providerMessage,
    };
  }
  return {
    kind,
    title: t('videoGeneration.workspace.failure.unknownTitle'),
    hint: t('videoGeneration.workspace.failure.unknownHint'),
    errorCode,
    providerMessage,
  };
}
