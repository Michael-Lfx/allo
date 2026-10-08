import {
  DEFAULT_WALLPAPER_PREFS,
  type WallpaperPrefs,
} from '@/common/types/appearance';

/** Retired CSS-gradient skins. Stored prefs that still name these are cleared. */
const LEGACY_PRESET_IDS = new Set(['dusk', 'dawn', 'midnight', 'paper', 'aurora', 'still-water']);

const clamp01 = (value: number, min: number, max: number): number =>
  Number.isFinite(value) ? Math.min(max, Math.max(min, value)) : min;

const libraryId = (value: unknown): string | null => {
  if (typeof value !== 'string') return null;
  const id = value.trim();
  if (!id || LEGACY_PRESET_IDS.has(id)) return null;
  return id;
};

type WallpaperPrefsInput = Partial<Omit<WallpaperPrefs, 'kind'>> & {
  kind?: string;
};

export const normalizeWallpaperPrefs = (raw: WallpaperPrefsInput | undefined | null): WallpaperPrefs => {
  if (!raw || typeof raw !== 'object') {
    return { ...DEFAULT_WALLPAPER_PREFS };
  }
  const dim =
    raw.dim === 'auto'
      ? 'auto'
      : typeof raw.dim === 'number'
        ? clamp01(raw.dim, 0, 0.8)
        : DEFAULT_WALLPAPER_PREFS.dim;
  const blur =
    raw.blur === 'auto'
      ? 'auto'
      : typeof raw.blur === 'number'
        ? clamp01(raw.blur, 0, 24)
        : DEFAULT_WALLPAPER_PREFS.blur;
  const id = libraryId(raw.id);
  const lightId = libraryId(raw.lightId);
  const darkId = libraryId(raw.darkId);
  const hasLibraryId = Boolean(id || lightId || darkId);
  return {
    enabled: Boolean(raw.enabled) && hasLibraryId,
    kind: 'library',
    id,
    lightId,
    darkId,
    fit: raw.fit === 'contain' ? 'contain' : 'cover',
    position: typeof raw.position === 'string' && raw.position.trim() ? raw.position : 'center',
    dim,
    blur,
    harmonizeAccent: Boolean(raw.harmonizeAccent),
    reducedMotionPolicy: raw.reducedMotionPolicy === 'play' ? 'play' : 'freeze',
    videoEnabled: Boolean(raw.videoEnabled),
  };
};

export const resolvedWallpaperId = (
  prefs: WallpaperPrefs,
  scheme: 'light' | 'dark'
): string | null => {
  if (prefs.id) return prefs.id;
  if (scheme === 'light' && prefs.lightId) return prefs.lightId;
  if (scheme === 'dark' && prefs.darkId) return prefs.darkId;
  return prefs.lightId ?? prefs.darkId ?? null;
};
