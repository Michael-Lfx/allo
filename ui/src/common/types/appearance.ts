/**
 * Wallpaper scene axis — independent of light/dark and CSS skins.
 *
 * Prefs store ids and knobs only. Bitmap bytes live on disk and are served
 * over HTTP, never inlined into SQLite or custom CSS.
 */

import type { WallpaperId } from '@/common/types/ids';

export type WallpaperFit = 'cover' | 'contain';
export type WallpaperKind = 'library';
export type WallpaperMediaKind = 'still' | 'animated' | 'video';
export type WallpaperReducedMotionPolicy = 'freeze' | 'play';
export type WallpaperDim = number | 'auto';
export type WallpaperBlur = number | 'auto';

export interface WallpaperPrefs {
  enabled: boolean;
  kind: WallpaperKind;
  /** Library UUIDv7. */
  id: string | null;
  lightId?: string | null;
  darkId?: string | null;
  fit: WallpaperFit;
  position: string;
  /** 0–0.8 when numeric; `auto` uses ingest recommendation. */
  dim: WallpaperDim;
  /** Pixel radius; `auto` uses ingest recommendation. */
  blur: WallpaperBlur;
  harmonizeAccent: boolean;
  reducedMotionPolicy: WallpaperReducedMotionPolicy;
  videoEnabled: boolean;
}

export interface WallpaperAnalysis {
  version: number;
  seedHex: string;
  meanLstar: number;
  luminanceVariance: number;
  recommendedScheme: 'light' | 'dark' | string;
  recommendedDim: number;
  recommendedBlurPx: number;
  recommendedPlateAlpha: number;
  primaryRgb: [number, number, number];
  primaryScale: [number, number, number][];
  busy: boolean;
  animated: boolean;
}

export interface WallpaperMeta {
  wallpaperId: WallpaperId;
  name: string;
  mediaKind: WallpaperMediaKind;
  originalExt: string;
  width: number;
  height: number;
  createdAt: number;
  analysis: WallpaperAnalysis;
}

export const DEFAULT_WALLPAPER_PREFS: WallpaperPrefs = {
  enabled: false,
  kind: 'library',
  id: null,
  lightId: null,
  darkId: null,
  fit: 'cover',
  position: 'center',
  dim: 'auto',
  blur: 'auto',
  harmonizeAccent: false,
  reducedMotionPolicy: 'freeze',
  videoEnabled: false,
};

export const WALLPAPER_CACHE_KEY = '__nomifun_wallpaper';
