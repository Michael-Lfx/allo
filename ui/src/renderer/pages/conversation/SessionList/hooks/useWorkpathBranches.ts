import { ipcBridge } from '@/common';
import { useEffect, useRef, useState, type RefObject } from 'react';

const BRANCH_CACHE_TTL_MS = 30_000;
const MAX_CONCURRENT_BRANCH_LOOKUPS = 4;

type BranchCacheEntry = {
  branch: string | null;
  expiresAt: number;
};

type BranchQueueEntry = {
  workpath: string;
  resolve: (branch: string | null) => void;
};

const branchCache = new Map<string, BranchCacheEntry>();
const branchInFlight = new Map<string, Promise<string | null>>();
const branchQueue: BranchQueueEntry[] = [];
let activeBranchLookups = 0;

function gitMetadataPath(workpath: string): string {
  const trimmed = workpath.replace(/[\\/]+$/, '');
  const separator = trimmed.includes('\\') && !trimmed.includes('/') ? '\\' : '/';
  return `${trimmed}${separator}.git`;
}

const readWorkpathBranch = async (workpath: string): Promise<string | null> => {
  try {
    await ipcBridge.fs.getFileMetadata.invoke({ path: gitMetadataPath(workpath), workspace: workpath });
  } catch {
    return null;
  }

  try {
    const info = await ipcBridge.fileSnapshot.init.invoke({ workspace: workpath });
    if (info.mode !== 'disabled') {
      void ipcBridge.fileSnapshot.dispose.invoke({ workspace: workpath }).catch(() => {});
    }
    return info.mode === 'git-repo' && info.branch ? info.branch : null;
  } catch {
    return null;
  }
};

const drainBranchQueue = (): void => {
  while (activeBranchLookups < MAX_CONCURRENT_BRANCH_LOOKUPS && branchQueue.length > 0) {
    const entry = branchQueue.shift();
    if (!entry) return;

    activeBranchLookups += 1;
    void readWorkpathBranch(entry.workpath)
      .catch(() => null)
      .then((branch) => {
        branchCache.set(entry.workpath, {
          branch,
          expiresAt: Date.now() + BRANCH_CACHE_TTL_MS,
        });
        entry.resolve(branch);
      })
      .finally(() => {
        activeBranchLookups -= 1;
        branchInFlight.delete(entry.workpath);
        drainBranchQueue();
      });
  }
};

const loadWorkpathBranch = (workpath: string): Promise<string | null> => {
  const cached = branchCache.get(workpath);
  if (cached && cached.expiresAt > Date.now()) return Promise.resolve(cached.branch);

  const existing = branchInFlight.get(workpath);
  if (existing) return existing;

  const promise = new Promise<string | null>((resolve) => {
    branchQueue.push({ workpath, resolve });
  });
  branchInFlight.set(workpath, promise);
  drainBranchQueue();
  return promise;
};

type UseWorkpathBranchResult = {
  branch: string | null;
  workpathRef: RefObject<HTMLDivElement | null>;
};

/**
 * Loads a workpath branch only when its drawer approaches the visible sidebar
 * viewport. Results are shared for a short period and queued to prevent a
 * large workspace tree from opening an IPC/snapshot storm on mount.
 */
export function useWorkpathBranch(workpath: string, enabled: boolean): UseWorkpathBranchResult {
  const workpathRef = useRef<HTMLDivElement | null>(null);
  const [branch, setBranch] = useState<string | null>(() => branchCache.get(workpath)?.branch ?? null);
  const [isNearViewport, setIsNearViewport] = useState(false);

  useEffect(() => {
    if (!enabled) {
      setIsNearViewport(false);
      return undefined;
    }

    const element = workpathRef.current;
    if (!element || typeof IntersectionObserver === 'undefined') {
      setIsNearViewport(true);
      return undefined;
    }

    const observer = new IntersectionObserver(
      ([entry]) => {
        if (!entry?.isIntersecting) return;
        setIsNearViewport(true);
        observer.disconnect();
      },
      { root: null, rootMargin: '200px 0px' }
    );
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, [enabled]);

  useEffect(() => {
    if (!enabled || !isNearViewport || !workpath) return undefined;

    let cancelled = false;
    void loadWorkpathBranch(workpath).then((nextBranch) => {
      if (!cancelled) setBranch(nextBranch);
    });

    return () => {
      cancelled = true;
    };
  }, [enabled, isNearViewport, workpath]);

  return { branch, workpathRef };
}
