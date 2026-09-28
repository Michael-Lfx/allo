import { useEffect, useState } from 'react';

import {
  acquireCachedArtifactMediaUrl,
  releaseCachedArtifactMediaUrl,
} from '../api';

export function useShotCanvasMedia(sessionId: string, paths: Array<string | null | undefined>) {
  const signature = paths
    .filter((path): path is string => Boolean(path))
    .map((path) => path.replace(/\\/g, '/'))
    .filter((path, index, all) => all.indexOf(path) === index)
    .join('\n');
  const [urls, setUrls] = useState<Record<string, string>>({});

  useEffect(() => {
    const unique = signature ? signature.split('\n') : [];
    if (!sessionId || unique.length === 0) {
      setUrls({});
      return;
    }
    let cancelled = false;
    const loaned: string[] = [];
    void Promise.all(
      unique.map(async (path) => {
        try {
          const url = await acquireCachedArtifactMediaUrl(sessionId, path);
          loaned.push(path);
          return [path, url] as const;
        } catch {
          return [path, ''] as const;
        }
      })
    ).then((entries) => {
      if (cancelled) {
        for (const path of loaned) releaseCachedArtifactMediaUrl(sessionId, path);
        return;
      }
      const next: Record<string, string> = {};
      for (const [path, url] of entries) {
        if (url) next[path] = url;
      }
      setUrls(next);
    });
    return () => {
      cancelled = true;
      for (const path of loaned) releaseCachedArtifactMediaUrl(sessionId, path);
    };
  }, [sessionId, signature]);

  return urls;
}
