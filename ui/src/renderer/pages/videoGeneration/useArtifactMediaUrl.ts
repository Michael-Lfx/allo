import { useCallback, useEffect, useState } from 'react';

import {
  acquireCachedArtifactMediaUrl,
  invalidateCachedArtifactMediaUrl,
  releaseCachedArtifactMediaUrl,
} from './api';

/**
 * Load a vimax artifact as a display URL and keep its blob loaned so the
 * shared LRU cannot revoke it while this component is mounted.
 */
export function useArtifactMediaUrl(sessionId: string | undefined, path: string | null | undefined) {
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const [epoch, setEpoch] = useState(0);

  useEffect(() => {
    if (!sessionId || !path) {
      setUrl(null);
      setFailed(false);
      return;
    }
    let cancelled = false;
    let loaned = false;
    setFailed(false);
    void acquireCachedArtifactMediaUrl(sessionId, path)
      .then((next) => {
        if (cancelled) {
          releaseCachedArtifactMediaUrl(sessionId, path);
          return;
        }
        loaned = true;
        setUrl(next);
      })
      .catch(() => {
        if (!cancelled) {
          setUrl(null);
          setFailed(true);
        }
      });
    return () => {
      cancelled = true;
      if (loaned) releaseCachedArtifactMediaUrl(sessionId, path);
    };
  }, [epoch, path, sessionId]);

  const reload = useCallback(() => {
    if (sessionId && path) invalidateCachedArtifactMediaUrl(sessionId, path);
    setEpoch((value) => value + 1);
  }, [path, sessionId]);

  return { url, failed, reload };
}
