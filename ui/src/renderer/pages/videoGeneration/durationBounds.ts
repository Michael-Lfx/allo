export const DURATION_MIN_SECS = 5;
export const DURATION_MAX_SECS = 300;
export const DURATION_STEP_SECS = 5;

/** Single-clip duration for Canvas / creation mode (Seedance-style). */
export const CLIP_DURATION_MIN_SECS = 4;
export const CLIP_DURATION_MAX_SECS = 15;
export const CLIP_DURATION_STEP_SECS = 1;
export const CLIP_DURATION_DEFAULT_SECS = 6;

export const AGENT_TICKS = [15, 30, 60, 90, 120, 180, 240, 300] as const;
export const CLIP_TICKS = [4, 6, 8, 10, 12, 15] as const;

/** Spoken briefing length (engine clamps to the same 30–300s range). */
export const BRIEFING_DURATION_MIN_SECS = 30;
export const BRIEFING_DURATION_MAX_SECS = 300;
export const BRIEFING_DURATION_STEP_SECS = 15;
export const BRIEFING_DURATION_DEFAULT_SECS = 90;
export const BRIEFING_TICKS = [30, 60, 90, 120, 180, 240, 300] as const;

export function clampDuration(
  secs: number,
  min = DURATION_MIN_SECS,
  max = DURATION_MAX_SECS,
  step = DURATION_STEP_SECS
): number {
  if (!Number.isFinite(secs)) {
    return Math.min(max, Math.max(min, min <= 30 && max >= 30 ? 30 : min));
  }
  const stepped = Math.round(secs / step) * step;
  return Math.min(max, Math.max(min, stepped));
}
