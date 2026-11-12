import { addEventListener, emitter } from '@/renderer/utils/emitter';

/**
 * Same-window theme events.
 *
 * These replace the former DOM events `custom-css-updated` and
 * `nomifun:css-themes-changed`. Every in-repo producer and consumer now goes
 * through this module; the Tauri `nomifun://theme-sync` channel stays separate
 * because it carries cross-window traffic. External scripts that still listen
 * for the old DOM event names must be updated separately.
 */

export interface ThemeCustomCssUpdatedPayload {
  /** Active ambiance preset CSS. An empty string means "clear custom CSS". */
  customCss: string;
}

/**
 * Notify this window's theme consumers that the persisted custom CSS changed.
 * Delivery is synchronous and ordered; a value of `''` is a valid update.
 */
export function dispatchThemeCustomCssUpdated(customCss: string): void {
  emitter.emit('theme.custom-css.updated', { customCss });
}

function reportListenerError(error: unknown): void {
  const reportError = (globalThis as { reportError?: (error: unknown) => void }).reportError;
  if (typeof reportError === 'function') {
    reportError.call(globalThis, error);
    return;
  }
  console.error('[themeEvents] Theme event listener failed:', error);
}

/**
 * Subscribe to same-window custom CSS updates. Each callback is isolated: a
 * throwing listener is reported and never blocks later listeners (or the
 * sender, which must not treat a listener failure as a failed config write).
 */
export function subscribeThemeCustomCssUpdated(
  listener: (payload: ThemeCustomCssUpdatedPayload) => void
): () => void {
  return addEventListener('theme.custom-css.updated', (payload) => {
    try {
      listener(payload);
    } catch (error) {
      reportListenerError(error);
    }
  });
}

/**
 * Notify this window's theme pickers that the user theme list changed.
 * Delivery is synchronous and ordered, with no payload.
 */
export function dispatchThemeCssThemesChanged(): void {
  emitter.emit('theme.css-themes.changed');
}

/** Subscribe to user theme list changes with per-listener error isolation. */
export function subscribeThemeCssThemesChanged(listener: () => void): () => void {
  return addEventListener('theme.css-themes.changed', () => {
    try {
      listener();
    } catch (error) {
      reportListenerError(error);
    }
  });
}
