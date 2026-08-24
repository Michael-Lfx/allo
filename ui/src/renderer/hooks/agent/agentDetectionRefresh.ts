/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export const AGENT_AUTO_REFRESH_MIN_INTERVAL_MS = 30_000;

/** Companion and memory-panel webviews load the same SPA entry point. */
export const shouldScheduleAgentRefreshForHash = (hash: string): boolean => {
  const route = hash.split('?')[0];
  const isAuxiliaryRoute =
    route === '#/companion' ||
    route.startsWith('#/companion/') ||
    route === '#/nomi-memory-panel' ||
    route.startsWith('#/nomi-memory-panel/') ||
    route === '#/completion-toast' ||
    route.startsWith('#/completion-toast/');
  return !isAuxiliaryRoute;
};

export const shouldScheduleAgentRefreshAfterHashChange = (previousHash: string, nextHash: string): boolean => {
  return !shouldScheduleAgentRefreshForHash(previousHash) && shouldScheduleAgentRefreshForHash(nextHash);
};

type AgentRefreshSchedulerOptions = {
  task: () => Promise<void>;
  intervalMs?: number;
  now?: () => number;
  onError?: (error: unknown) => void;
};

type AgentAvailabilityRefreshOptions<T> = {
  refreshSnapshot: () => Promise<T>;
  replaceCachedSnapshot: (snapshot: T) => Promise<unknown>;
};

/** Refresh the backend registry, then replace the shared client snapshot. */
export const refreshAgentAvailability = async <T>({
  refreshSnapshot,
  replaceCachedSnapshot,
}: AgentAvailabilityRefreshOptions<T>): Promise<void> => {
  const snapshot = await refreshSnapshot();
  await replaceCachedSnapshot(snapshot);
};

/**
 * Build a refresh action that shares an in-flight probe and rate-limits later
 * probes. The caller decides when to schedule it, so the action never blocks
 * a component mount by itself.
 */
export const createAgentRefreshScheduler = ({
  task,
  intervalMs = AGENT_AUTO_REFRESH_MIN_INTERVAL_MS,
  now = Date.now,
  onError = () => {},
}: AgentRefreshSchedulerOptions): (() => Promise<void>) => {
  let lastRefreshAt: number | null = null;
  let inFlight: Promise<void> | null = null;

  return async () => {
    if (inFlight) return inFlight;

    const currentTime = now();
    if (lastRefreshAt !== null && currentTime - lastRefreshAt < intervalMs) return;

    lastRefreshAt = currentTime;
    inFlight = task()
      .catch((error) => {
        onError(error);
      })
      .finally(() => {
        inFlight = null;
      });
    return inFlight;
  };
};
