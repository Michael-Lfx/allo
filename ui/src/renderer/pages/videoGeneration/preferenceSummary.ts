import type { GenerationPreferences, VideoHomeMode } from './home/types';
import type { VimaxWorkflow } from './types';

export function generationPreferencesSummary(
  value: GenerationPreferences,
  mode: VideoHomeMode,
  labels: { automatic: string; smart: string; noModel: string },
  workflow?: VimaxWorkflow
): { summary: string; title: string } {
  const rawModel =
    value.mediaKind === 'image' ? value.models.image_model : value.models.video_model;
  const modelTail = rawModel.trim().split(/[/:@]/).pop() || rawModel.trim();
  const modelLabel = modelTail
    ? modelTail.length > 14
      ? `${modelTail.slice(0, 12)}…`
      : modelTail
    : labels.noModel;
  if (mode === 'action' || workflow === 'action2video') {
    const summary = `${modelLabel} · ${value.resolution.toUpperCase()}`;
    return { summary, title: `video · ${summary}` };
  }
  if (mode === 'generate') {
    const ratio = value.smartAspect ? labels.smart : value.aspectRatio;
    const summary = `${ratio} · ${value.resolution.toUpperCase()} · ${value.targetDurationSecs}s`;
    return { summary, title: `video · ${summary}` };
  }
  const ratio = value.smartAspect ? labels.smart : value.aspectRatio;
  const media = value.mediaKind === 'image' ? modelLabel : value.resolution.toUpperCase();
  const summary = value.automatic
    ? labels.automatic
    : value.mediaKind === 'image'
      ? `${ratio} · ${modelLabel}`
      : `${ratio} · ${value.resolution.toUpperCase()}`;
  const title = value.automatic
    ? labels.automatic
    : `${value.mediaKind === 'image' ? 'image' : 'video'} · ${ratio} · ${
        value.mediaKind === 'video'
          ? value.resolution.toUpperCase()
          : media
      }`;
  return { summary, title };
}
