/**
 * Pure view-model for the 「即刻出片」 detail page.
 * Timestamps, frame shape, and regenerate payloads stay here so the page
 * cannot drift back into the bugs this module exists to prevent.
 */
import { aspectRatioNumber } from './aspectRatios';
import { toUpdatedAtMs } from './recentCreations';

export type ClipTaskStatus = 'queued' | 'running' | 'succeeded' | 'failed' | 'canceled';

export type ClipFollowUp = 'regenerate' | 'resume';

export type ClipStepKey = 'queued' | 'initializing' | 'rendering' | 'finalizing';

export type ClipRegenerateBody = {
  mode: string;
  prompt: string;
  model?: string;
  aspect_ratio?: string;
  resolution?: string;
  duration_secs?: number;
  reference_media_ids?: string[];
  audio_media_ids?: string[];
  reference_video_media_id?: string;
  first_frame_media_id?: string;
  last_frame_media_id?: string;
};

/** Epoch milliseconds. Canvas tasks already store millis; older rows may be seconds. */
export function clipTimestampMs(value: number | null | undefined): number | null {
  const ms = toUpdatedAtMs(value);
  return ms > 0 ? ms : null;
}

/**
 * Width/height for the preview frame.
 * A measured video ratio wins, so portrait clips are not locked to 16:9.
 */
export function clipFrameAspect(
  videoAspect: number | null | undefined,
  taskAspect: string | null | undefined
): number {
  if (
    typeof videoAspect === 'number' &&
    Number.isFinite(videoAspect) &&
    videoAspect >= 0.2 &&
    videoAspect <= 4
  ) {
    return videoAspect;
  }
  return aspectRatioNumber(taskAspect);
}

/**
 * Backend progress is 0..1. Zero means "no measurement yet" (indeterminate),
 * not a filled bar stuck at 0%. Values above 1 are treated as percents.
 */
export function clipProgressPercent(progress: number | null | undefined): number | null {
  if (typeof progress !== 'number' || !Number.isFinite(progress) || progress <= 0) return null;
  const ratio = progress > 1 ? progress / 100 : progress;
  if (ratio <= 0) return null;
  return Math.min(100, Math.max(1, Math.round(ratio * 100)));
}

/**
 * Failed, canceled, and finished clips start a new task.
 * Credits failures and in-flight jobs only refresh the current one.
 */
export function clipFollowUp(
  status: ClipTaskStatus | null | undefined,
  options?: { credits?: boolean }
): ClipFollowUp {
  if (options?.credits) return 'resume';
  if (status === 'succeeded' || status === 'failed' || status === 'canceled') return 'regenerate';
  return 'resume';
}

export function clipStepKey(
  status: ClipTaskStatus | null | undefined,
  percent: number | null
): ClipStepKey {
  if (status !== 'running') return 'queued';
  if (percent == null || percent < 12) return 'initializing';
  if (percent < 88) return 'rendering';
  return 'finalizing';
}

export function shortClipModel(model: string | null | undefined): string {
  const raw = (model ?? '').trim();
  if (!raw) return '';
  return raw.split(/[/:@]/).pop()?.trim() || raw;
}

export function clipRegenerateBody(task: {
  mode?: string | null;
  prompt?: string | null;
  model?: string | null;
  aspect_ratio?: string | null;
  resolution?: string | null;
  duration_secs?: number | null;
  reference_media_ids?: string[] | null;
  audio_media_ids?: string[] | null;
  reference_video_media_id?: string | null;
  first_frame_media_id?: string | null;
  last_frame_media_id?: string | null;
}): ClipRegenerateBody | null {
  const prompt = (task.prompt ?? '').trim();
  if (!prompt) return null;
  const text = (value?: string | null) => {
    const next = value?.trim();
    return next ? next : undefined;
  };
  const ids = (value?: string[] | null) => {
    const list = (value ?? []).map((id) => id.trim()).filter(Boolean);
    return list.length ? list : undefined;
  };
  return {
    mode: text(task.mode) || 'video',
    prompt,
    model: text(task.model),
    aspect_ratio: text(task.aspect_ratio),
    resolution: text(task.resolution),
    duration_secs:
      typeof task.duration_secs === 'number' && task.duration_secs > 0
        ? task.duration_secs
        : undefined,
    reference_media_ids: ids(task.reference_media_ids),
    audio_media_ids: ids(task.audio_media_ids),
    reference_video_media_id: text(task.reference_video_media_id),
    first_frame_media_id: text(task.first_frame_media_id),
    last_frame_media_id: text(task.last_frame_media_id),
  };
}

/** First frame, references, then last frame. Duplicates are dropped. */
export function clipStillIds(task: {
  first_frame_media_id?: string | null;
  last_frame_media_id?: string | null;
  reference_media_ids?: string[] | null;
}): string[] {
  const ordered = [
    task.first_frame_media_id,
    ...(task.reference_media_ids ?? []),
    task.last_frame_media_id,
  ];
  const seen = new Set<string>();
  const ids: string[] = [];
  for (const id of ordered) {
    const next = id?.trim();
    if (!next || seen.has(next)) continue;
    seen.add(next);
    ids.push(next);
    if (ids.length === 6) break;
  }
  return ids;
}
