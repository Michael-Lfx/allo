import type { GenerationPreferences, VideoHomeMode } from './home/types';
import type { VimaxWorkflow } from './types';
import {
  CLIP_DURATION_MAX_SECS,
  CLIP_DURATION_MIN_SECS,
  CLIP_DURATION_STEP_SECS,
  clampDuration,
  DURATION_MAX_SECS,
  DURATION_MIN_SECS,
  DURATION_STEP_SECS,
} from './durationBounds';

export function generationPreferencesSummary(
  value: GenerationPreferences,
  mode: VideoHomeMode,
  labels: { automatic: string; smart: string; noModel: string },
  workflow?: VimaxWorkflow
): { summary: string; title: string } {
  const durationSecs = clampDuration(
    value.targetDurationSecs,
    mode === 'creation' ? CLIP_DURATION_MIN_SECS : DURATION_MIN_SECS,
    mode === 'creation' ? CLIP_DURATION_MAX_SECS : DURATION_MAX_SECS,
    mode === 'creation' ? CLIP_DURATION_STEP_SECS : DURATION_STEP_SECS
  );
  const rawModel =
    value.mediaKind === 'image' ? value.models.image_model : value.models.video_model;
  const modelTail = rawModel.trim().split(/[/:@]/).pop() || rawModel.trim();
  const modelLabel = modelTail
    ? modelTail.length > 14
      ? `${modelTail.slice(0, 12)}…`
      : modelTail
    : labels.noModel;
  if (workflow === 'action2video') {
    const summary = `${modelLabel} · ${value.resolution.toUpperCase()}`;
    return { summary, title: `video · ${summary}` };
  }
  const ratio = value.smartAspect ? labels.smart : value.aspectRatio;
  const media = value.mediaKind === 'image' ? modelLabel : value.resolution.toUpperCase();
  const summary = value.automatic
    ? labels.automatic
    : value.mediaKind === 'image'
      ? `${ratio} · ${modelLabel}`
      : mode === 'agent'
        ? `${ratio} · ${durationSecs}s · ${value.resolution.toUpperCase()}`
        : `${ratio} · ${value.resolution.toUpperCase()}`;
  const title = value.automatic
    ? labels.automatic
    : `${value.mediaKind === 'image' ? 'image' : 'video'} · ${ratio} · ${
        value.mediaKind === 'video'
          ? mode === 'agent'
            ? `${durationSecs}s · ${value.resolution.toUpperCase()}`
            : value.resolution.toUpperCase()
          : media
      }`;
  return { summary, title };
}
