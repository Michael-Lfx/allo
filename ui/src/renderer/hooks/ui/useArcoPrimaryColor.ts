import { useCallback, useLayoutEffect, useState } from 'react';
import { useThemeContext } from '@/renderer/hooks/context/ThemeContext';
import { useAddEventListener } from '@/renderer/utils/emitter';

const LIGHT_FALLBACK = '#1d1d1f';
const DARK_FALLBACK = '#f5f5f7';

/**
 * Resolve Arco's primary color from the active Flowy semantic token.
 *
 * Arco needs a concrete color value when it derives its palette. Reading the
 * already-applied CSS token in a layout effect keeps that bridge out of render
 * and lets custom themes update the bridge without introducing a second color
 * source. The Flowy primary button intentionally stays neutral; teal remains
 * reserved for accent/focus/send states.
 */
export const useArcoPrimaryColor = (): string => {
  const { theme, colorScheme } = useThemeContext();
  const fallback = theme === 'dark' ? DARK_FALLBACK : LIGHT_FALLBACK;
  const [primaryColor, setPrimaryColor] = useState(fallback);

  const refresh = useCallback(() => {
    if (typeof document === 'undefined') {
      setPrimaryColor(fallback);
      return;
    }

    const root = document.documentElement;
    const token = getComputedStyle(root).getPropertyValue('--flowy-ink-button').trim();
    setPrimaryColor(token || fallback);
  }, [fallback]);

  useLayoutEffect(() => {
    refresh();
  }, [refresh, colorScheme]);

  useAddEventListener('theme.custom-css.updated', refresh, [refresh]);

  return primaryColor;
};

