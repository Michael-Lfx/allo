/**
 * Debounced canvas document persistence.
 */

import { useEffect, useRef } from 'react';
import type { CanvasDocument } from '../types';
import { putCanvasDoc } from '../api';

export function useCanvasDocAutosave(
  projectId: string | null,
  doc: CanvasDocument | null,
  enabled: boolean
) {
  const timerRef = useRef<number | null>(null);
  const lastSavedRef = useRef<string>('');

  useEffect(() => {
    if (!enabled || !projectId || !doc) return;
    const serialized = JSON.stringify(doc);
    if (serialized === lastSavedRef.current) return;

    if (timerRef.current != null) {
      window.clearTimeout(timerRef.current);
    }
    timerRef.current = window.setTimeout(() => {
      void putCanvasDoc(projectId, doc)
        .then(() => {
          lastSavedRef.current = serialized;
        })
        .catch((e) => {
          console.error('[videoCanvas] autosave failed', e);
        });
    }, 800);

    return () => {
      if (timerRef.current != null) window.clearTimeout(timerRef.current);
    };
  }, [projectId, doc, enabled]);
}
