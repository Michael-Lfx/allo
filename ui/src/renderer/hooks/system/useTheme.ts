// hooks/useTheme.ts
import { configService } from '@/common/config/configService';
import { broadcastThemeSync } from '@renderer/utils/theme/themeBroadcast';
import { useCallback, useEffect, useRef, useState } from 'react';

/**
 * Resolved light/dark scheme actually applied to the document.
 * `data-theme` / `arco-theme` only ever receive one of these two values —
 * the 'system' preference is resolved before anything touches the DOM.
 */
export type Theme = 'light' | 'dark';

/**
 * User-selected theme preference. 'system' means "follow the OS color scheme"
 * and is the only value that is persisted; it is resolved to a concrete
 * {@link Theme} via `prefers-color-scheme` before being applied.
 */
export type ThemePreference = 'light' | 'dark' | 'system';

// Default to following the OS for fresh installs. Existing users already have a
// concrete 'light'/'dark' persisted, so this never overrides their choice.
const DEFAULT_PREFERENCE: ThemePreference = 'system';
const THEME_CACHE_KEY = '__nomifun_theme';
const MEDIA_DARK = '(prefers-color-scheme: dark)';

const systemPrefersDark = (): boolean =>
  typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? window.matchMedia(MEDIA_DARK).matches
    : false;

/** Resolve a preference to the concrete light/dark scheme to apply. */
export const resolveTheme = (preference: ThemePreference): Theme =>
  preference === 'system' ? (systemPrefersDark() ? 'dark' : 'light') : preference;

const applyThemeToDom = (resolved: Theme) => {
  document.documentElement.setAttribute('data-theme', resolved);
  document.body.setAttribute('arco-theme', resolved);
};

const isPreference = (value: string): value is ThemePreference =>
  value === 'light' || value === 'dark' || value === 'system';

export { isPreference };

const readCachedPreference = (): ThemePreference => {
  try {
    const cached = localStorage.getItem(THEME_CACHE_KEY);
    if (cached && isPreference(cached)) return cached;
  } catch (_e) {
    /* noop */
  }
  return DEFAULT_PREFERENCE;
};

// Apply localStorage hint synchronously to avoid FOUC, then resolve to the
// authoritative value from configService once it has loaded from the backend.
const initTheme = async (): Promise<ThemePreference> => {
  const hint = readCachedPreference();
  applyThemeToDom(resolveTheme(hint));
  try {
    await configService.whenReady();
    const stored = configService.get('theme');
    const preference = typeof stored === 'string' && isPreference(stored) ? stored : hint;
    applyThemeToDom(resolveTheme(preference));
    try {
      localStorage.setItem(THEME_CACHE_KEY, preference);
    } catch (_e) {
      /* noop */
    }
    return preference;
  } catch (error) {
    console.error('Failed to load initial theme:', error);
    return hint;
  }
};

// Run theme initialization immediately
let initialThemePromise: Promise<ThemePreference> | null = null;
if (typeof window !== 'undefined') {
  initialThemePromise = initTheme();
}

const useTheme = (): [Theme, ThemePreference, (preference: ThemePreference) => Promise<void>] => {
  const [preference, setPreferenceState] = useState<ThemePreference>(readCachedPreference);
  const [theme, setThemeState] = useState<Theme>(() => resolveTheme(preference));

  // Mirror the latest preference into a ref so the OS-scheme listener below can
  // bail out if the user picks an explicit scheme in the gap between their click
  // and React tearing the listener down (cleanup runs post-commit, so a 'change'
  // event landing in that window would otherwise re-apply the stale 'system'
  // resolution over the user's just-selected explicit theme).
  const preferenceRef = useRef(preference);

  // Apply preference to document (resolving 'system') and cache it for FOUC
  const applyPreference = useCallback((next: ThemePreference) => {
    applyThemeToDom(resolveTheme(next));
    try {
      localStorage.setItem(THEME_CACHE_KEY, next);
    } catch (_e) {
      /* noop */
    }
  }, []);

  // Set preference with persistence. Broadcasts the RESOLVED light/dark so
  // companion windows apply it without each having to re-evaluate the OS scheme.
  const setTheme = useCallback(
    async (next: ThemePreference) => {
      const previous = preference;
      try {
        preferenceRef.current = next;
        setPreferenceState(next);
        setThemeState(resolveTheme(next));
        applyPreference(next);
        await configService.set('theme', next);
        // 仅在持久化成功后广播：失败会走 catch 回滚，避免给独立窗口（桌宠）
        // 广播一个最终被回滚的值导致跨窗短暂不一致。
        broadcastThemeSync(resolveTheme(next));
      } catch (error) {
        console.error('Failed to save theme:', error);
        // Revert on error
        preferenceRef.current = previous;
        setPreferenceState(previous);
        setThemeState(resolveTheme(previous));
        applyPreference(previous);
      }
    },
    [preference, applyPreference]
  );

  // Initialize theme state from the early initialization
  useEffect(() => {
    if (initialThemePromise) {
      initialThemePromise
        .then((initialPreference) => {
          preferenceRef.current = initialPreference;
          setPreferenceState(initialPreference);
          setThemeState(resolveTheme(initialPreference));
        })
        .catch((error) => {
          console.error('Failed to initialize theme:', error);
        });
    }
  }, []);

  // Follow the OS color scheme live while the preference is 'system'. When the
  // user picks an explicit light/dark the listener is torn down, so manual
  // choices are never overridden by the OS.
  useEffect(() => {
    if (preference !== 'system') return;
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return;
    const media = window.matchMedia(MEDIA_DARK);
    if (typeof media.addEventListener !== 'function') return;
    const onChange = () => {
      // Bail if the user switched to an explicit scheme in the gap between
      // their click and this listener's teardown — their choice wins.
      if (preferenceRef.current !== 'system') return;
      const resolved = resolveTheme('system');
      setThemeState(resolved);
      applyThemeToDom(resolved);
      broadcastThemeSync(resolved);
    };
    media.addEventListener('change', onChange);
    return () => media.removeEventListener('change', onChange);
  }, [preference]);

  return [theme, preference, setTheme];
};

export default useTheme;
