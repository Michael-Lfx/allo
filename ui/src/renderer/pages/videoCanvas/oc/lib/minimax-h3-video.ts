/**
 * MiniMax-H3 (Flowy / MiniMax V2) canvas-side video options.
 * Model detection and resolution normalization live in
 * `@renderer/services/videoModelCapabilities` (shared with videoGeneration).
 * Keep resolution tokens in sync with Rust `normalize_minimax_h3_resolution`.
 */

export const MINIMAX_H3_RATIOS = [
  { value: '16:9', label: '横屏' },
  { value: '9:16', label: '竖屏' },
  { value: '1:1', label: '方形' },
  { value: '4:3', label: '标准横屏' },
  { value: '3:4', label: '标准竖屏' },
  { value: '21:9', label: '宽银幕' },
] as const;

/** Duration seconds accepted by MiniMax-H3 create API. */
export const MINIMAX_H3_DURATION_MIN = 4;
export const MINIMAX_H3_DURATION_MAX = 15;
export const MINIMAX_H3_DURATION_DEFAULT = 5;

export function normalizeMiniMaxH3Duration(value: string | number): number {
  const n = typeof value === 'number' ? value : Number.parseInt(String(value), 10);
  if (!Number.isFinite(n)) return MINIMAX_H3_DURATION_DEFAULT;
  return Math.min(MINIMAX_H3_DURATION_MAX, Math.max(MINIMAX_H3_DURATION_MIN, Math.round(n)));
}

/** Text-to-video ratio: never `adaptive`. Image/multimodal may use adaptive upstream. */
export function normalizeMiniMaxH3Ratio(value: string, hasMediaRefs: boolean): string {
  if (hasMediaRefs) return 'adaptive';
  const raw = String(value || '').trim();
  if (!raw || raw === 'auto' || raw === 'adaptive') return '16:9';
  if (MINIMAX_H3_RATIOS.some((item) => item.value === raw)) return raw;
  return '16:9';
}
