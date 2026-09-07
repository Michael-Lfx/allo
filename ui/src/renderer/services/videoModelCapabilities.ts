/**
 * Per-model video resolution / fps / duration capabilities shared by videoGeneration
 * (ViMax Style & Model pickers) and videoCanvas oc panels.
 * Heuristics mirror Rust `nomi_vimax::video_quality` (Seedance Ark + MiniMax-H3 + Wan 3.0).
 */

export const VIDEO_RESOLUTIONS = ['480p', '720p', '1080p'] as const;
export type SeedanceVideoResolution = (typeof VIDEO_RESOLUTIONS)[number];

/** MiniMax-H3 create API resolutions (canonical casing). */
export const MINIMAX_H3_RESOLUTIONS = ['768P', '2K'] as const;
export type MiniMaxH3VideoResolution = (typeof MINIMAX_H3_RESOLUTIONS)[number];

/** Wan 3.0 DashScope `parameters.resolution` (canonical casing). */
export const WAN3_RESOLUTIONS = ['480P', '720P', '1080P'] as const;
export type Wan3VideoResolution = (typeof WAN3_RESOLUTIONS)[number];

export type VideoResolution =
  | SeedanceVideoResolution
  | MiniMaxH3VideoResolution
  | Wan3VideoResolution
  | string;

export const DEFAULT_VIDEO_RESOLUTION: SeedanceVideoResolution = '720p';
export const DEFAULT_MINIMAX_H3_RESOLUTION: MiniMaxH3VideoResolution = '768P';
export const DEFAULT_WAN3_RESOLUTION: Wan3VideoResolution = '720P';
export const DEFAULT_VIDEO_FPS = 24;

export interface VideoModelCapabilities {
  resolutions: VideoResolution[];
  fpsOptions: number[];
  /** When true the UI shows fps but disables changing it. */
  fpsLocked: boolean;
  durationMin: number;
  durationMax: number;
  durationDefault: number;
}

function modelBlob(model: string): string {
  return model.toLowerCase().replace(/[_.\s/]/g, '-');
}

export function isMiniMaxH3VideoModel(model: string): boolean {
  const b = modelBlob(model);
  return (
    b.includes('minimax-h3') ||
    b.includes('minimaxh3') ||
    (b.includes('minimax') && b.includes('h3'))
  );
}

export function isWan3VideoModel(model: string): boolean {
  const b = modelBlob(model);
  return b.includes('wan3') || b.includes('wan-3-0');
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

export function normalizeWan3Resolution(resolution: string): Wan3VideoResolution {
  const lower = resolution.trim().toLowerCase().replace(/[_\s]/g, '');
  if (['1080p', '1080', '2k', '2160p', '2160', '4k', 'high'].includes(lower)) return '1080P';
  if (['480p', '480', 'low'].includes(lower)) return '480P';
  if (['720p', '720', 'medium', 'auto'].includes(lower)) return DEFAULT_WAN3_RESOLUTION;
  const exact = WAN3_RESOLUTIONS.find((r) => r.toLowerCase() === lower);
  return exact ?? DEFAULT_WAN3_RESOLUTION;
}

export function videoModelCapabilities(model: string): VideoModelCapabilities {
  if (isMiniMaxH3VideoModel(model)) {
    return {
      resolutions: [...MINIMAX_H3_RESOLUTIONS],
      fpsOptions: [DEFAULT_VIDEO_FPS],
      fpsLocked: true,
      durationMin: 4,
      durationMax: 15,
      durationDefault: 5,
    };
  }
  if (isWan3VideoModel(model)) {
    return {
      resolutions: [...WAN3_RESOLUTIONS],
      fpsOptions: [DEFAULT_VIDEO_FPS],
      fpsLocked: true,
      durationMin: 2,
      durationMax: 30,
      durationDefault: 5,
    };
  }
  if (isSeedanceFastOrMini(model)) {
    return {
      resolutions: ['480p', '720p'],
      fpsOptions: [DEFAULT_VIDEO_FPS],
      fpsLocked: true,
      durationMin: 5,
      durationMax: 15,
      durationDefault: 6,
    };
  }
  if (isSeedance(model)) {
    return {
      resolutions: ['480p', '720p', '1080p'],
      fpsOptions: [DEFAULT_VIDEO_FPS],
      fpsLocked: true,
      durationMin: 5,
      durationMax: 15,
      durationDefault: 6,
    };
  }
  return {
    resolutions: [...VIDEO_RESOLUTIONS],
    fpsOptions: [DEFAULT_VIDEO_FPS],
    fpsLocked: true,
    durationMin: 4,
    durationMax: 15,
    durationDefault: 6,
  };
}

export function normalizeVideoResolution(model: string, resolution: string): VideoResolution {
  if (isMiniMaxH3VideoModel(model)) {
    return normalizeMiniMaxH3Resolution(resolution);
  }
  if (isWan3VideoModel(model)) {
    return normalizeWan3Resolution(resolution);
  }
  // Canvas UI may store bare heights (`1080`); Seedance / Flowy expect `1080p`.
  let raw = resolution.trim().toLowerCase().replace(/[_\s]/g, '');
  if (raw === 'low') raw = '480p';
  else if (raw === 'auto' || raw === 'medium' || raw === 'high') raw = '720p';
  else if (raw === '4k' || raw === '2160' || raw === '2160p') raw = '2160p';
  else if (raw && !raw.endsWith('p') && /^\d+$/.test(raw)) raw = `${raw}p`;

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
