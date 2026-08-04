/** Rough Seedance / film render cost used for pre-flight credit estimates. */
export const CREDITS_PER_SECOND = 900;

/** Estimate credits from target film duration (seconds). */
export function estimateDurationCredits(secs: number): number {
  const n = Number.isFinite(secs) ? Math.max(0, Math.round(secs)) : 0;
  return n * CREDITS_PER_SECOND;
}

/** Locale-friendly credit count for UI labels. */
export function formatDurationCredits(secs: number): string {
  return estimateDurationCredits(secs).toLocaleString();
}
