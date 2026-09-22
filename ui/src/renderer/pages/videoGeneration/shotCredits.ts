import type { SessionStatus } from './types';
import {
  parseSceneFromMessage,
  parseShotFromMessage,
  sceneIndexFromRoot,
  statusMetaNumber,
} from './storyboardVideoStatus';

type CreditEvent = NonNullable<SessionStatus['events']>[number];

const SCENE_STAGES = new Set([
  'render_scene',
  'render_scene_skip',
  'render_scene_done',
  'render_scene_failed',
]);

const SHOT_STAGES = new Set(['video_clip_start', 'video_clip_done', 'video_clip_exists']);

/** Stable map key: scene-aware when `scene_N` is known, else shot-only. */
export function shotCreditKey(
  sceneRoot: string | undefined,
  shotIndex: number | null | undefined
): string | null {
  if (shotIndex == null || !Number.isFinite(shotIndex)) return null;
  return eventShotKey(sceneIndexFromRoot(sceneRoot), shotIndex);
}

function eventShotKey(sceneIndex: number | null, shotIndex: number | null): string | null {
  if (shotIndex == null || !Number.isFinite(shotIndex)) return null;
  return sceneIndex == null ? `s:${shotIndex}` : `c:${sceneIndex}:s:${shotIndex}`;
}

/**
 * Walk the live event log and attribute each terminal `video_credits` bill
 * to the shot that was generating at the time (or `shot_idx` on the event).
 */
export function creditsByShotFromSessionEvents(
  events: CreditEvent[] | null | undefined
): Map<string, number> {
  const byKey = new Map<string, number>();
  let currentShot: number | null = null;
  let currentScene: number | null = null;

  for (const event of events ?? []) {
    if (SCENE_STAGES.has(event.stage)) {
      const scene =
        statusMetaNumber(event.metadata, 'scene_idx') ?? parseSceneFromMessage(event.message);
      if (scene != null) currentScene = scene;
    }
    if (SHOT_STAGES.has(event.stage)) {
      const shot =
        statusMetaNumber(event.metadata, 'shot_idx') ?? parseShotFromMessage(event.message);
      if (shot != null) currentShot = shot;
    }
    if (event.stage !== 'video_credits') continue;
    const credits = statusMetaNumber(event.metadata, 'credits_consumed');
    if (credits == null || credits <= 0) continue;
    const shot = statusMetaNumber(event.metadata, 'shot_idx') ?? currentShot;
    const scene = statusMetaNumber(event.metadata, 'scene_idx') ?? currentScene;
    const key = eventShotKey(scene, shot);
    if (!key) continue;
    byKey.set(key, Math.max(byKey.get(key) ?? 0, credits));
  }
  return byKey;
}

export function parseShotCreditsFile(text: string | undefined): number {
  if (!text?.trim()) return 0;
  try {
    const parsed = JSON.parse(text) as unknown;
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return 0;
    const credits = Number((parsed as { credits_consumed?: unknown }).credits_consumed);
    return Number.isFinite(credits) && credits > 0 ? credits : 0;
  } catch {
    return 0;
  }
}

export function resolveShotCreditsConsumed(options: {
  sceneRoot?: string;
  shotIndex?: number;
  hasVideo: boolean;
  eventCredits: Map<string, number>;
  sidecarCredits: Map<string, number>;
}): number {
  if (!options.hasVideo) return 0;
  const key = shotCreditKey(options.sceneRoot, options.shotIndex);
  if (!key) return 0;
  return Math.max(options.eventCredits.get(key) ?? 0, options.sidecarCredits.get(key) ?? 0);
}
