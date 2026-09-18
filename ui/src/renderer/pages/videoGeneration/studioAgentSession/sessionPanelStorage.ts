const VERSION = 'v2';
const RATIO_KEY = `studioAgentSession:widthRatio:${VERSION}`;
const COLLAPSED_KEY = `studioAgentSession:collapsed:v1`;

/** Comfortable storyboard / film column — session grows only from leftover. */
export const STUDIO_MAIN_MIN_WIDTH = 720;
export const STUDIO_SESSION_WIDTH_DEFAULT = 360;
export const STUDIO_SESSION_WIDTH_MIN = 320;
export const STUDIO_SESSION_WIDTH_MAX = 560;
/** ~360px at a 1280px studio shell. */
export const STUDIO_SESSION_WIDTH_RATIO_DEFAULT = STUDIO_SESSION_WIDTH_DEFAULT / 1280;

function readStorage(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStorage(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Quota / private mode.
  }
}

export function loadStudioSessionWidthRatio(): number {
  const raw = readStorage(RATIO_KEY);
  const n = raw ? Number(raw) : NaN;
  if (!Number.isFinite(n) || n <= 0 || n >= 1) return STUDIO_SESSION_WIDTH_RATIO_DEFAULT;
  return n;
}

export function saveStudioSessionWidthRatio(ratio: number): void {
  if (!Number.isFinite(ratio) || ratio <= 0) return;
  writeStorage(RATIO_KEY, String(ratio));
}

export function maxStudioSessionWidth(shellWidth: number): number {
  if (!(shellWidth > 0)) return STUDIO_SESSION_WIDTH_DEFAULT;
  return Math.min(
    STUDIO_SESSION_WIDTH_MAX,
    Math.max(STUDIO_SESSION_WIDTH_MIN, shellWidth - STUDIO_MAIN_MIN_WIDTH)
  );
}

export function computeStudioSessionWidth(shellWidth: number, ratio: number): number {
  const cap = maxStudioSessionWidth(shellWidth);
  const fromRatio = (shellWidth > 0 ? shellWidth : 1280) * (ratio > 0 ? ratio : STUDIO_SESSION_WIDTH_RATIO_DEFAULT);
  return Math.round(Math.min(cap, Math.max(STUDIO_SESSION_WIDTH_MIN, fromRatio)));
}

export function clampStudioSessionWidth(width: number, shellWidth: number): number {
  const cap = maxStudioSessionWidth(shellWidth);
  return Math.round(Math.min(cap, Math.max(STUDIO_SESSION_WIDTH_MIN, width)));
}

export function loadStudioSessionCollapsed(): boolean {
  return readStorage(COLLAPSED_KEY) === '1';
}

export function saveStudioSessionCollapsed(collapsed: boolean): void {
  writeStorage(COLLAPSED_KEY, collapsed ? '1' : '0');
}
