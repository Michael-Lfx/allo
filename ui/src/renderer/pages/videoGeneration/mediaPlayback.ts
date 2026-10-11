/** Incomplete WebKitGTK/WPE builds can expose `<video>` without media methods. */

type PlayableMedia = {
  play?: () => Promise<void> | void;
};

type PausableMedia = {
  pause?: () => void;
};

export function playMediaElement(media: PlayableMedia | null | undefined): void {
  if (!media || typeof media.play !== 'function') return;
  try {
    const result = media.play();
    if (result && typeof result.catch === 'function') {
      void result.catch(() => undefined);
    }
  } catch {
    // Autoplay policy and missing media backends both surface here.
  }
}

export function pauseMediaElement(media: PausableMedia | null | undefined): void {
  if (!media || typeof media.pause !== 'function') return;
  try {
    media.pause();
  } catch {
    // Same incomplete-media-element path as playMediaElement.
  }
}
