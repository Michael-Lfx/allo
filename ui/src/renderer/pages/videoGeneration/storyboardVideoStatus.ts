/**
 * Derive which storyboard shot is currently generating video from live
 * session status (progress events + stage metadata).
 */

import type { SessionStatus } from './types';

export type StoryboardVideoSlotStatus = 'ready' | 'generating' | 'pending';

export interface ActiveVideoGenerationTarget {
  /** Shot index within its pipeline scene (`shot.idx`). */
  shotIndex: number | null;
  /** 0-based scene index when multi-scene; null for single-scene / unknown. */
  sceneIndex: number | null;
}

function metaNumber(metadata: unknown, key: string): number | null {
  if (!metadata || typeof metadata !== 'object' || Array.isArray(metadata)) return null;
  const value = (metadata as Record<string, unknown>)[key];
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'string' && value.trim() !== '') {
    const n = Number(value);
    return Number.isFinite(n) ? n : null;
  }
  return null;
}

function parseShotFromMessage(message: string | null | undefined): number | null {
  const text = message?.trim() ?? '';
  if (!text) return null;
  const patterns = [
    /镜头\s*(\d+)/i,
    /shot\s+(\d+)/i,
    /Generating shot\s+(\d+)/i,
    /Shot\s+(\d+)\b/i,
  ];
  for (const pattern of patterns) {
    const match = text.match(pattern);
    if (match) return Number(match[1]);
  }
  return null;
}

function parseSceneFromMessage(message: string | null | undefined): number | null {
  const text = message?.trim() ?? '';
  if (!text) return null;
  // "正在渲染场景（1/2）" / "Scene 1/2" — 1-based in copy → convert to 0-based.
  const sceneMatch =
    text.match(/场景\s*[（(]?\s*(\d+)\s*[/／]/i) ||
    text.match(/Scene\s+(\d+)\s*\//i) ||
    text.match(/事件\s+\d+\s+场景\s+(\d+)/i);
  if (sceneMatch) {
    const n = Number(sceneMatch[1]);
    return Number.isFinite(n) ? Math.max(0, n - 1) : null;
  }
  return null;
}

function sceneIndexFromRoot(sceneRoot: string | undefined): number | null {
  if (!sceneRoot) return null;
  const normalized = sceneRoot.replace(/\\/g, '/');
  const match = normalized.match(/scene_(\d+)/i);
  if (!match) return null;
  const n = Number(match[1]);
  return Number.isFinite(n) ? n : null;
}

/** Latest generating shot/scene while a render job is active. */
export function activeVideoGenerationTarget(
  status: SessionStatus | null | undefined
): ActiveVideoGenerationTarget {
  if (!status || status.status !== 'rendering') {
    return { shotIndex: null, sceneIndex: null };
  }

  const events = [...(status.events ?? [])].reverse();

  let sceneIndex: number | null =
    metaNumber(
      events.find((ev) =>
        ['render_scene', 'render_scene_skip', 'render_scene_done', 'render_scene_failed'].includes(
          ev.stage
        )
      )?.metadata,
      'scene_idx'
    ) ??
    parseSceneFromMessage(
      events.find((ev) =>
        ['render_scene', 'render_scene_skip', 'render_scene_done', 'render_scene_failed'].includes(
          ev.stage
        )
      )?.message
    ) ??
    parseSceneFromMessage(status.message);

  // Clip just finished — wait for the next video_clip_start.
  if (status.stage === 'video_clip_done' || status.stage === 'video_clip_exists') {
    return { shotIndex: null, sceneIndex };
  }

  const generatingStages = new Set([
    'video_clip_start',
    'video_generate',
    'video_clips_start',
    'video_continuity',
    // Fine-grained Seedance job stages (still the same shot).
    'video_create',
    'video_poll',
    'video_download',
  ]);

  // Only mark a shot as generating while the pipeline is actually in a clip stage.
  if (!generatingStages.has(status.stage)) {
    return { shotIndex: null, sceneIndex };
  }

  let shotIndex: number | null = parseShotFromMessage(status.message);

  for (const ev of events) {
    if (!generatingStages.has(ev.stage) && ev.stage !== 'video_clip_done') continue;
    const fromMeta = metaNumber(ev.metadata, 'shot_idx');
    const fromMsg = parseShotFromMessage(ev.message);
    if (fromMeta != null || fromMsg != null) {
      // Prefer structured metadata from the newest generating event.
      shotIndex = fromMeta ?? fromMsg ?? shotIndex;
      break;
    }
  }

  return { shotIndex, sceneIndex };
}

export function resolveStoryboardVideoStatus(options: {
  hasVideo: boolean;
  shotIndex?: number;
  sceneRoot?: string;
  rendering: boolean;
  target: ActiveVideoGenerationTarget;
}): StoryboardVideoSlotStatus {
  if (options.hasVideo) return 'ready';
  if (!options.rendering) return 'pending';

  const { shotIndex, sceneRoot, target } = options;
  if (target.shotIndex == null || shotIndex == null) {
    // Generic render phase without a resolved shot — keep pending so we don't
    // light up every empty cell as "generating".
    return 'pending';
  }
  if (target.shotIndex !== shotIndex) return 'pending';

  if (target.sceneIndex != null) {
    const rootScene = sceneIndexFromRoot(sceneRoot);
    if (rootScene != null && rootScene !== target.sceneIndex) return 'pending';
  }

  return 'generating';
}
