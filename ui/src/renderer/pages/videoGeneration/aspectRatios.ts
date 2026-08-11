/**
 * Seedance 2.0 aspect ratios shared by Style & models + MediaSettings.
 * Keep in sync with `nomi_media_backends::aspect::SEEDANCE_ASPECT_RATIOS`.
 */
export const SEEDANCE_ASPECT_RATIOS = ['16:9', '9:16', '1:1', '4:3', '3:4', '21:9'] as const;
export type SeedanceAspectRatio = (typeof SEEDANCE_ASPECT_RATIOS)[number];

export const DEFAULT_SEEDANCE_ASPECT_RATIO: SeedanceAspectRatio = '16:9';

export function normalizeSeedanceAspectRatio(raw: string | null | undefined): SeedanceAspectRatio {
  const t = (raw ?? '').trim().replace('：', ':');
  const hit = SEEDANCE_ASPECT_RATIOS.find((r) => r.toLowerCase() === t.toLowerCase());
  return hit ?? DEFAULT_SEEDANCE_ASPECT_RATIO;
}

export function seedanceAspectSelectOptions(): { label: string; value: SeedanceAspectRatio }[] {
  return SEEDANCE_ASPECT_RATIOS.map((value) => ({ label: value, value }));
}
