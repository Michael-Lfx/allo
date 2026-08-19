/**
 * Per-model resolution / fps capabilities for ViMax Style & Model pickers.
 * Heuristics mirror Rust `nomi_vimax::video_quality` (Seedance Ark + MiniMax-H3).
 */

export const VIDEO_RESOLUTIONS = ['480p', '720p', '1080p'] as const;
export type SeedanceVideoResolution = (typeof VIDEO_RESOLUTIONS)[number];

/** MiniMax-H3 create API resolutions (canonical casing). */
export const MINIMAX_H3_RESOLUTIONS = ['768P', '2K'] as const;
export type MiniMaxH3VideoResolution = (typeof MINIMAX_H3_RESOLUTIONS)[number];

export type VideoResolution = SeedanceVideoResolution | MiniMaxH3VideoResolution | string;

export const DEFAULT_VIDEO_RESOLUTION: SeedanceVideoResolution = '720p';
export const DEFAULT_MINIMAX_H3_RESOLUTION: MiniMaxH3VideoResolution = '768P';
export const DEFAULT_VIDEO_FPS = 24;

export interface VideoModelCapabilities {
  resolutions: VideoResolution[];
  fpsOptions: number[];
  /** When true the UI shows fps but disables changing it. */
  fpsLocked: boolean;
}

function modelBlob(model: string): string {
  return model.toLowerCase().replace(/[_.\s/]/g, '-');
}

export function isMiniMaxH3VideoModel(model: string): boolean {
  const b = modelBlob(model);
  return b.includes('minimax-h3') || b.includes('minimaxh3');
}

function isSeedance(model: string): boolean {
  return modelBlob(model).includes('seedance');
}

function isSeedanceFastOrMini(model: string): boolean {
  const b = modelBlob(model);
  return b.includes('seedance') && (b.includes('fast') || b.includes('mini'));
}

export function normalizeMiniMaxH3Resolution(resolution: string): MiniMaxH3VideoResolution {
  const lower = resolution.trim().toLowerCase().replace(/[_\s]/g, '');
  if (['2k', '1080p', '1080', '2160p', '4k', 'high'].includes(lower)) return '2K';
  if (MINIMAX_H3_RESOLUTIONS.some((r) => r.toLowerCase() === lower)) {
    return (MINIMAX_H3_RESOLUTIONS.find((r) => r.toLowerCase() === lower) ??
      DEFAULT_MINIMAX_H3_RESOLUTION) as MiniMaxH3VideoResolution;
  }
  return DEFAULT_MINIMAX_H3_RESOLUTION;
}

export function videoModelCapabilities(model: string): VideoModelCapabilities {
  if (isMiniMaxH3VideoModel(model)) {
    return {
      resolutions: [...MINIMAX_H3_RESOLUTIONS],
      fpsOptions: [DEFAULT_VIDEO_FPS],
      fpsLocked: true,
    };
  }
  if (isSeedanceFastOrMini(model)) {
    return {
      resolutions: ['480p', '720p'],
      fpsOptions: [DEFAULT_VIDEO_FPS],
      fpsLocked: true,
    };
  }
  if (isSeedance(model)) {
    return {
      resolutions: ['480p', '720p', '1080p'],
      fpsOptions: [DEFAULT_VIDEO_FPS],
      fpsLocked: true,
    };
  }
  return {
    resolutions: [...VIDEO_RESOLUTIONS],
    fpsOptions: [DEFAULT_VIDEO_FPS],
    fpsLocked: true,
  };
}

export function normalizeVideoResolution(model: string, resolution: string): VideoResolution {
  if (isMiniMaxH3VideoModel(model)) {
    return normalizeMiniMaxH3Resolution(resolution);
  }
  const raw = resolution.trim().toLowerCase();
  const caps = videoModelCapabilities(model);
  if (caps.resolutions.includes(raw as SeedanceVideoResolution)) {
    return raw as SeedanceVideoResolution;
  }
  if (caps.resolutions.includes(DEFAULT_VIDEO_RESOLUTION)) {
    return DEFAULT_VIDEO_RESOLUTION;
  }
  return caps.resolutions[caps.resolutions.length - 1] ?? DEFAULT_VIDEO_RESOLUTION;
}

export function normalizeVideoFps(model: string, fps: number): number {
  const caps = videoModelCapabilities(model);
  if (caps.fpsOptions.includes(fps)) return fps;
  return caps.fpsOptions[0] ?? DEFAULT_VIDEO_FPS;
}

export function defaultVideoResolutionForModel(model: string): VideoResolution {
  return isMiniMaxH3VideoModel(model) ? DEFAULT_MINIMAX_H3_RESOLUTION : DEFAULT_VIDEO_RESOLUTION;
}
