import { useEffect, useMemo, useRef, useState } from 'react';

import {
  acquireCachedArtifactMediaUrl,
  releaseCachedArtifactMediaUrl,
} from '../api';

export function uniqueShotMediaPaths(paths: Array<string | null | undefined>): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const path of paths) {
    if (!path) continue;
    const normalized = path.replace(/\\/g, '/');
    if (seen.has(normalized)) continue;
    seen.add(normalized);
    out.push(normalized);
  }
  return out;
}

/** Keep URLs that are still needed so shared refs (定妆 / 场景) stay on screen while a shot swap loads. */
export function retainShotMediaUrls(
  current: Record<string, string>,
  wanted: readonly string[]
): Record<string, string> {
  const keep = new Set(wanted);
  let changed = false;
  const next: Record<string, string> = {};
  for (const [path, url] of Object.entries(current)) {
    if (!keep.has(path)) {
      changed = true;
      continue;
    }
    next[path] = url;
  }
  if (!changed && Object.keys(next).length === Object.keys(current).length) return current;
  return next;
}

export function useShotCanvasMedia(sessionId: string, paths: Array<string | null | undefined>) {
  const unique = useMemo(() => uniqueShotMediaPaths(paths), [paths]);
  const signature = unique.join('\n');
  const [urls, setUrls] = useState<Record<string, string>>({});
  const wantedRef = useRef(new Set<string>());
  const heldRef = useRef(new Set<string>());
  const inflightRef = useRef(new Map<string, number>());
  const aliveRef = useRef(true);
  const sessionRef = useRef(sessionId);
  sessionRef.current = sessionId;

  wantedRef.current = new Set(unique);

  useEffect(() => {
    const wanted = signature ? signature.split('\n').filter(Boolean) : [];
    setUrls((current) => retainShotMediaUrls(current, wanted));
  }, [signature]);

  useEffect(() => {
    if (!sessionId) {
      setUrls({});
      return;
    }

    const wanted = signature ? signature.split('\n').filter(Boolean) : [];
    const wantedSet = new Set(wanted);
    const held = heldRef.current;

    for (const path of [...held]) {
      if (wantedSet.has(path)) continue;
      held.delete(path);
      releaseCachedArtifactMediaUrl(sessionId, path);
    }

    const begin = (path: string) => {
      inflightRef.current.set(path, (inflightRef.current.get(path) ?? 0) + 1);
      void acquireCachedArtifactMediaUrl(sessionId, path)
        .then((url) => {
          const pending = inflightRef.current.get(path) ?? 0;
          if (pending <= 1) inflightRef.current.delete(path);
          else inflightRef.current.set(path, pending - 1);

          if (!aliveRef.current || sessionRef.current !== sessionId || !wantedRef.current.has(path)) {
            releaseCachedArtifactMediaUrl(sessionId, path);
            return;
          }
          held.add(path);
          if (!url) return;
          setUrls((current) => (current[path] === url ? current : { ...current, [path]: url }));
        })
        .catch(() => {
          const pending = inflightRef.current.get(path) ?? 0;
          if (pending <= 1) inflightRef.current.delete(path);
          else inflightRef.current.set(path, pending - 1);
        });
    };

    for (const path of wanted) {
      if (held.has(path) || inflightRef.current.has(path)) continue;
      begin(path);
    }
  }, [sessionId, signature]);

  useEffect(() => {
    aliveRef.current = true;
    return () => {
      aliveRef.current = false;
      const held = heldRef.current;
      for (const path of held) releaseCachedArtifactMediaUrl(sessionId, path);
      held.clear();
      inflightRef.current.clear();
    };
  }, [sessionId]);

  return urls;
}
