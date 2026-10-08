import { WALLPAPER_CACHE_KEY } from '@/common/types/appearance';
import { SEND_BUTTON_GUARD_STYLE_ID } from './sendButtonGuardCss';
import {
  WALLPAPER_TOKENS_STYLE_ID,
  buildWallpaperTokenCss,
  wallpaperFoucCache,
  type WallpaperTokenInput,
} from './wallpaperTokens';

function mountWallpaperSheet(styleEl: HTMLStyleElement): void {
  const guard = document.getElementById(SEND_BUTTON_GUARD_STYLE_ID);
  if (guard) {
    guard.before(styleEl);
  } else {
    document.head.appendChild(styleEl);
  }
}

export function applyWallpaperTokens(input: WallpaperTokenInput): string {
  if (typeof document === 'undefined') return '';

  const css = buildWallpaperTokenCss(input);
  const enabled = Boolean(input.enabled && input.analysis);
  if (enabled) {
    document.documentElement.setAttribute('data-wallpaper', 'on');
  } else {
    document.documentElement.removeAttribute('data-wallpaper');
  }
  document.documentElement.style.removeProperty('background-color');

  const existing = document.getElementById(WALLPAPER_TOKENS_STYLE_ID);
  if (!css) {
    existing?.remove();
  } else if (existing) {
    if (existing.textContent !== css) existing.textContent = css;
  } else {
    const styleEl = document.createElement('style');
    styleEl.id = WALLPAPER_TOKENS_STYLE_ID;
    styleEl.type = 'text/css';
    styleEl.textContent = css;
    mountWallpaperSheet(styleEl);
  }

  try {
    localStorage.setItem(
      WALLPAPER_CACHE_KEY,
      JSON.stringify({ ...wallpaperFoucCache(input), css })
    );
  } catch {
    /* quota / private mode */
  }
  return css;
}

export function injectWallpaperTokenSheet(css: string, enabled: boolean): void {
  if (typeof document === 'undefined') return;
  if (enabled && css) {
    document.documentElement.setAttribute('data-wallpaper', 'on');
  } else {
    document.documentElement.removeAttribute('data-wallpaper');
  }
  document.documentElement.style.removeProperty('background-color');
  const existing = document.getElementById(WALLPAPER_TOKENS_STYLE_ID);
  if (!css) {
    existing?.remove();
    return;
  }
  if (existing) {
    if (existing.textContent !== css) existing.textContent = css;
    return;
  }
  const styleEl = document.createElement('style');
  styleEl.id = WALLPAPER_TOKENS_STYLE_ID;
  styleEl.type = 'text/css';
  styleEl.textContent = css;
  mountWallpaperSheet(styleEl);
}

export function restoreWallpaperTokensFromCache(): void {
  try {
    const raw = localStorage.getItem(WALLPAPER_CACHE_KEY);
    if (!raw) return;
    const parsed = JSON.parse(raw) as { enabled?: boolean; css?: string };
    injectWallpaperTokenSheet(typeof parsed.css === 'string' ? parsed.css : '', Boolean(parsed.enabled));
  } catch {
    /* ignore */
  }
}
