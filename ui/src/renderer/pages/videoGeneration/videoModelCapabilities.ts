/**
 * Per-model resolution / fps capabilities for ViMax Style & Model pickers.
 * Heuristics mirror Rust `nomi_vimax::video_quality` (Seedance Ark docs).
 */

export const VIDEO_RESOLUTIONS = ['480p', '720p', '1080p'] as const;
export type VideoResolution = (typeof VIDEO_RESOLUTIONS)[number];

export const DEFAULT_VIDEO_RESOLUTION: VideoResolution = '720p';
export const DEFAULT_VIDEO_FPS = 24;

export interface VideoModelCapabilities {
  resolutions: VideoResolution[];
  fpsOptions: number[];
  /** When true the UI shows fps but disables changing it. */
  fpsLocked: boolean;
}

function modelBlob(model: string): string {
  return model.toLowerCase().replace(/[_.\s]/g, '-');
}

function isSeedance(model: string): boolean {
  return modelBlob(model).includes('seedance');
}

function isSeedanceFastOrMini(model: string): boolean {
  const b = modelBlob(model);
  return b.includes('seedance') && (b.includes('fast') || b.includes('mini'));
}

export function videoModelCapabilities(model: string): VideoModelCapabilities {
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
  const raw = resolution.trim().toLowerCase();
  const caps = videoModelCapabilities(model);
  if (caps.resolutions.includes(raw as VideoResolution)) {
    return raw as VideoResolution;
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
