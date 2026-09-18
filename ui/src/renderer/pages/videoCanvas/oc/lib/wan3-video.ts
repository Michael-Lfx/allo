/**
 * Wan 3.0 (DashScope) canvas-side video options.
 * Model detection and resolution normalization live in
 * `@renderer/services/videoModelCapabilities` (shared with videoGeneration).
 * Keep tokens in sync with Rust `normalize_wan3_resolution` / `clamp_wan3_duration`.
 */

export const WAN3_DURATION_MIN = 2;
export const WAN3_DURATION_MAX = 30;
export const WAN3_DURATION_DEFAULT = 5;

export function normalizeWan3Duration(value: string | number): number {
  const n = typeof value === 'number' ? value : Number.parseInt(String(value), 10);
  if (!Number.isFinite(n)) return WAN3_DURATION_DEFAULT;
  return Math.min(WAN3_DURATION_MAX, Math.max(WAN3_DURATION_MIN, Math.round(n)));
}

/** Text-to-video ratio: never `adaptive`. Image/multimodal uses adaptive upstream. */
export function normalizeWan3Ratio(value: string, hasMediaRefs: boolean): string {
  if (hasMediaRefs) return 'adaptive';
  const raw = String(value || '').trim();
  if (!raw || raw === 'auto' || raw === 'adaptive') return '16:9';
  return raw;
}
