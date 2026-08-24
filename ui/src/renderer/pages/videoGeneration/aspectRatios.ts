/**
 * Seedance 2.0 aspect ratios shared by Style & models + MediaSettings.
 * Keep in sync with `nomi_vimax::aspect::SEEDANCE_ASPECT_RATIOS`.
 */
export const SEEDANCE_ASPECT_RATIOS = ['16:9', '9:16', '1:1', '4:3', '3:4', '21:9'] as const;
export type SeedanceAspectRatio = (typeof SEEDANCE_ASPECT_RATIOS)[number];

export const DEFAULT_SEEDANCE_ASPECT_RATIO: SeedanceAspectRatio = '16:9';

export function normalizeSeedanceAspectRatio(raw: string | null | undefined): SeedanceAspectRatio {
  const t = (raw ?? '').trim().replace('：', ':');
  const hit = SEEDANCE_ASPECT_RATIOS.find((r) => r.toLowerCase() === t.toLowerCase());
  return hit ?? DEFAULT_SEEDANCE_ASPECT_RATIO;
}

/** Unitless width/height for CSS `aspect-ratio` / `--shot-aspect`. */
export function aspectRatioNumber(raw: string | null | undefined): number {
  const n = normalizeSeedanceAspectRatio(raw);
  const [w, h] = n.split(':').map(Number);
  if (!w || !h) return 16 / 9;
  return w / h;
}
