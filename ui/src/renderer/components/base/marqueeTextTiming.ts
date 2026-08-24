export const MARQUEE_START_DELAY_MS = 600;
export const MARQUEE_END_PAUSE_MS = 900;
export const MARQUEE_RETURN_MS = 180;
export const MARQUEE_SPEED_PX_PER_SECOND = 32;
export const MARQUEE_MIN_TRAVEL_MS = 1320;
export const MARQUEE_MAX_TRAVEL_MS = 6900;

// A marquee should cover equal distances in equal time. Easing would make
// the text visibly accelerate or decelerate over a long path.
export const MARQUEE_EASE = 'linear';

export function getMarqueeTravelDuration(distancePx: number): number {
  if (!Number.isFinite(distancePx) || distancePx <= 0) return 0;

  const travelMs = Math.round((distancePx / MARQUEE_SPEED_PX_PER_SECOND) * 1000);
  return Math.min(MARQUEE_MAX_TRAVEL_MS, Math.max(MARQUEE_MIN_TRAVEL_MS, travelMs));
}

export function getMarqueeTotalDuration(distancePx: number): number {
  const travelMs = getMarqueeTravelDuration(distancePx);
  return travelMs === 0 ? 0 : travelMs + MARQUEE_END_PAUSE_MS + MARQUEE_RETURN_MS;
}
