/**
 * Fullscreen helpers for the clip result player.
 *
 * Tauri / WebView2 often rejects `Element.requestFullscreen()` on a page
 * wrapper, and even when it succeeds the player must be the fullscreen
 * target — otherwise the video stays letterboxed at `max-height: 70vh`.
 */

export type FullscreenHost = {
  requestFullscreen?: () => Promise<void> | void;
  webkitRequestFullscreen?: () => Promise<void> | void;
};

export type FullscreenDocument = {
  fullscreenElement?: Element | null;
  webkitFullscreenElement?: Element | null;
  fullscreenEnabled?: boolean;
  webkitFullscreenEnabled?: boolean;
  exitFullscreen?: () => Promise<void> | void;
  webkitExitFullscreen?: () => Promise<void> | void;
  addEventListener?: (type: string, listener: () => void) => void;
  removeEventListener?: (type: string, listener: () => void) => void;
};

function resolveDocument(root?: FullscreenDocument | null): FullscreenDocument | null {
  if (root) return root;
  if (typeof document === 'undefined') return null;
  return document;
}

export function getFullscreenElement(root?: FullscreenDocument | null): Element | null {
  const doc = resolveDocument(root);
  return doc?.fullscreenElement ?? doc?.webkitFullscreenElement ?? null;
}

export function isElementFullscreen(
  el: object | null | undefined,
  root?: FullscreenDocument | null
): boolean {
  return Boolean(el) && getFullscreenElement(root) === el;
}

export function canUseNativeFullscreen(
  el?: FullscreenHost | null,
  root?: FullscreenDocument | null
): boolean {
  const doc = resolveDocument(root);
  if (!doc || !el) return false;
  if (doc.fullscreenEnabled === false || doc.webkitFullscreenEnabled === false) {
    return false;
  }
  return (
    typeof el.requestFullscreen === 'function' ||
    typeof el.webkitRequestFullscreen === 'function'
  );
}

export async function requestElementFullscreen(
  el: FullscreenHost,
  root?: FullscreenDocument | null
): Promise<boolean> {
  if (!canUseNativeFullscreen(el, root)) return false;
  try {
    if (typeof el.requestFullscreen === 'function') {
      await el.requestFullscreen();
    } else if (typeof el.webkitRequestFullscreen === 'function') {
      await el.webkitRequestFullscreen();
    } else {
      return false;
    }
    return isElementFullscreen(el, root);
  } catch {
    return false;
  }
}

export async function exitElementFullscreen(root?: FullscreenDocument | null): Promise<void> {
  const doc = resolveDocument(root);
  if (!doc || !getFullscreenElement(doc)) return;
  try {
    if (typeof doc.exitFullscreen === 'function') {
      await doc.exitFullscreen();
      return;
    }
    if (typeof doc.webkitExitFullscreen === 'function') {
      await doc.webkitExitFullscreen();
    }
  } catch {
    /* already exited or denied */
  }
}

export function subscribeFullscreenChange(
  listener: () => void,
  root?: FullscreenDocument | null
): () => void {
  const doc = resolveDocument(root);
  const add = doc?.addEventListener;
  const remove = doc?.removeEventListener;
  if (!add || !remove) return () => undefined;
  add('fullscreenchange', listener);
  add('webkitfullscreenchange', listener);
  return () => {
    remove('fullscreenchange', listener);
    remove('webkitfullscreenchange', listener);
  };
}
