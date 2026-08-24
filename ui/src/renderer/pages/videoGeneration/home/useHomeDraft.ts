import { useEffect, useRef, useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import {
  DRAFT_KEY,
  HOME_DRAFT_DEBOUNCE_MS,
  createHomeDraftWriter,
  loadDraft,
  serializeDraft,
} from './homeDraft';
import type { HomeDraftWriter } from './homeDraft';
import type { VideoCreateDraft } from './types';

export interface HomeDraftApi {
  draft: VideoCreateDraft;
  setDraft: Dispatch<SetStateAction<VideoCreateDraft>>;
}

/**
 * Draft state plus debounced sessionStorage persistence. Writes are coalesced
 * within ~400ms and flushed on pagehide / visibility-hidden / unmount so a
 * refresh or navigation never loses the last change. Object URLs are revoked
 * on unmount.
 */
export function useHomeDraft(): HomeDraftApi {
  const [draft, setDraft] = useState<VideoCreateDraft>(loadDraft);
  const draftRef = useRef(draft);
  const writerRef = useRef<HomeDraftWriter | null>(null);
  if (writerRef.current === null) {
    writerRef.current = createHomeDraftWriter(() => {
      const current = draftRef.current;
      try {
        window.sessionStorage.setItem(
          DRAFT_KEY,
          JSON.stringify(serializeDraft(current))
        );
      } catch {
        // Storage may be unavailable in hardened webviews.
      }
    }, HOME_DRAFT_DEBOUNCE_MS);
  }

  useEffect(() => {
    draftRef.current = draft;
    writerRef.current?.markDirty();
  }, [draft]);

  useEffect(() => {
    const writer = writerRef.current;
    if (!writer) return undefined;
    const flushOnPageHide = () => {
      writer.flush();
    };
    const flushWhenHidden = () => {
      if (document.visibilityState === 'hidden') writer.flush();
    };
    window.addEventListener('pagehide', flushOnPageHide);
    document.addEventListener('visibilitychange', flushWhenHidden);
    return () => {
      window.removeEventListener('pagehide', flushOnPageHide);
      document.removeEventListener('visibilitychange', flushWhenHidden);
      // Unload-time flush before revoking object URLs: never lose the last edit.
      writer.dispose();
      const current = draftRef.current;
      for (const cameo of current.cameos) {
        if (cameo.previewUrl) URL.revokeObjectURL(cameo.previewUrl);
      }
      for (const reference of current.canvasReferences) {
        URL.revokeObjectURL(reference.previewUrl);
      }
      if (current.actionCharacter?.previewUrl) {
        URL.revokeObjectURL(current.actionCharacter.previewUrl);
      }
      if (current.actionVideo?.previewUrl) {
        URL.revokeObjectURL(current.actionVideo.previewUrl);
      }
    };
  }, []);

  return { draft, setDraft };
}
