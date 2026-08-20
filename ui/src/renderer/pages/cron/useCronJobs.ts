

import { ipcBridge } from '@/common';
import type { ICronJob, ICronJobRun, IUpdateCronJobParams } from '@/common/adapter/ipcBridge';
import {
  indexCronJobsByConversation,
  reconcileCronJobsForConversation,
} from './cronJobConversationMap';
import { parseConversationId, type ConversationId, type CronJobId } from '@/common/types/ids';
import { emitter } from '@/renderer/utils/emitter';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import useSWR, { type SWRConfiguration } from 'swr';
import { repairCronJobTimeZones } from '@renderer/pages/cron/repairCronJobTimeZone';
import { browserStorageGenerationKey } from '@/common/utils/browserStorageKey';

/** One cache identity for every all-jobs consumer (task page, create dialog, session list). */
export const ALL_CRON_JOBS_SWR_KEY = 'cron.jobs.all';

export const ALL_CRON_JOBS_SWR_OPTIONS: SWRConfiguration<ICronJob[], Error> = {
  revalidateIfStale: false,
  revalidateOnFocus: false,
  revalidateOnReconnect: false,
  shouldRetryOnError: false,
};

export async function fetchAllCronJobs(): Promise<ICronJob[]> {
  try {
    const allJobs = await ipcBridge.cron.listJobs.invoke();
    return repairCronJobTimeZones(allJobs || []);
  } catch (err) {
    console.error('[useAllCronJobs] Failed to fetch jobs:', err);
    throw err;
  }
}

const upsertCronJobInList = (jobs: ICronJob[], job: ICronJob): ICronJob[] =>
  jobs.some((item) => item.cron_job_id === job.cron_job_id)
    ? jobs.map((item) => (item.cron_job_id === job.cron_job_id ? job : item))
    : [...jobs, job];


const isJobErrorLike = (job: ICronJob): boolean => {
  return job.state.last_status === 'error' || job.state.last_status === 'missed';
};

/**
 * Common cron job actions
 */
interface CronJobActionsResult {
  pauseJob: (cron_job_id: CronJobId) => Promise<void>;
  resumeJob: (cron_job_id: CronJobId) => Promise<void>;
  deleteJob: (cron_job_id: CronJobId) => Promise<void>;
  updateJob: (cron_job_id: CronJobId, updates: IUpdateCronJobParams) => Promise<ICronJob>;
}

/**
 * Creates common cron job action handlers
 */
function useCronJobActions(
  onJobUpdated?: (cron_job_id: CronJobId, job: ICronJob) => void,
  onJobDeleted?: (cron_job_id: CronJobId) => void
): CronJobActionsResult {
  const pauseJob = useCallback(
    async (cron_job_id: CronJobId) => {
      const updated = await ipcBridge.cron.updateJob.invoke({ cron_job_id, updates: { enabled: false } });
      onJobUpdated?.(cron_job_id, updated);
    },
    [onJobUpdated]
  );

  const resumeJob = useCallback(
    async (cron_job_id: CronJobId) => {
      const updated = await ipcBridge.cron.updateJob.invoke({ cron_job_id, updates: { enabled: true } });
      onJobUpdated?.(cron_job_id, updated);
    },
    [onJobUpdated]
  );

  const deleteJob = useCallback(
    async (cron_job_id: CronJobId) => {
      await ipcBridge.cron.removeJob.invoke({ cron_job_id });
      onJobDeleted?.(cron_job_id);
    },
    [onJobDeleted]
  );

  const updateJob = useCallback(
    async (cron_job_id: CronJobId, updates: IUpdateCronJobParams) => {
      const updated = await ipcBridge.cron.updateJob.invoke({ cron_job_id, updates });
      onJobUpdated?.(cron_job_id, updated);
      return updated;
    },
    [onJobUpdated]
  );

  return { pauseJob, resumeJob, deleteJob, updateJob };
}

/**
 * Event handlers for cron job subscription
 */
interface CronJobEventHandlers {
  onJobCreated: (job: ICronJob) => void;
  onJobUpdated: (job: ICronJob) => void;
  onJobRemoved: (data: { cron_job_id: CronJobId }) => void;
}

/**
 * Subscribe to cron job events with unified cleanup.
 *
 * WebSocket delivery has no replay: any gap (reconnect, server lag resync)
 * may have dropped cron job events, so `onResync` reloads the caller's
 * durable snapshot after every reconnect.
 */
function useCronJobSubscription(handlers: CronJobEventHandlers, onResync?: () => void | Promise<void>) {
  useEffect(() => {
    const unsubCreate = ipcBridge.cron.onJobCreated.on(handlers.onJobCreated);
    const unsubUpdate = ipcBridge.cron.onJobUpdated.on(handlers.onJobUpdated);
    const unsubRemove = ipcBridge.cron.onJobRemoved.on(handlers.onJobRemoved);
    const unsubReconnected = onResync ? ipcBridge.conversation.reconnected.on(() => void onResync()) : undefined;

    return () => {
      unsubCreate();
      unsubUpdate();
      unsubRemove();
      unsubReconnected?.();
    };
  }, [handlers.onJobCreated, handlers.onJobUpdated, handlers.onJobRemoved, onResync]);
}

/**
 * Hook for managing cron jobs for a specific conversation
 * @param conversation_id - The conversation ID to fetch jobs for
 */
export function useCronJobs(conversation_id?: ConversationId) {
  const [jobs, setJobs] = useState<ICronJob[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<Error | null>(null);

  // Fetch jobs for the conversation
  const fetchJobs = useCallback(async () => {
    if (conversation_id == null) {
      setJobs([]);
      return;
    }

    setLoading(true);
    setError(null);

    try {
      const result = await ipcBridge.cron.listJobsByConversation.invoke({ conversation_id });
      setJobs(await repairCronJobTimeZones(result || []));
    } catch (err) {
      setError(err instanceof Error ? err : new Error('Failed to fetch cron jobs'));
      setJobs([]);
    } finally {
      setLoading(false);
    }
  }, [conversation_id]);

  // Initial fetch
  useEffect(() => {
    void fetchJobs();
  }, [fetchJobs]);

  // Event handlers
  const eventHandlers = useMemo<CronJobEventHandlers>(
    () => ({
      onJobCreated: (job: ICronJob) => {
        if (conversation_id && job.metadata.conversation_id === conversation_id) {
          setJobs((prev) => (prev.some((j) => j.cron_job_id === job.cron_job_id) ? prev : [...prev, job]));
        }
      },
      onJobUpdated: (job: ICronJob) => {
        if (!conversation_id) return;
        setJobs((prev) => reconcileCronJobsForConversation(prev, conversation_id, job));
      },
      onJobRemoved: ({ cron_job_id }: { cron_job_id: CronJobId }) => {
        setJobs((prev) => prev.filter((j) => j.cron_job_id !== cron_job_id));
      },
    }),
    [conversation_id]
  );

  useCronJobSubscription(eventHandlers, fetchJobs);

  // Actions (without local state updates, rely on events)
  const actions = useCronJobActions();

  // Computed values
  const hasJobs = jobs.length > 0;
  const activeJobsCount = jobs.filter((j) => j.enabled).length;
  const hasError = jobs.some(isJobErrorLike);

  return {
    jobs,
    loading,
    error,
    hasJobs,
    activeJobsCount,
    hasError,
    refetch: fetchJobs,
    ...actions,
  };
}

/**
 * Shared all-jobs snapshot. Session-list indicators and the scheduled-tasks
 * page must read the same SWR key so navigating home → /scheduled does not
 * refetch GET /api/cron/jobs.
 */
function useAllCronJobsCache() {
  const { data, isLoading, mutate } = useSWR<ICronJob[]>(
    ALL_CRON_JOBS_SWR_KEY,
    fetchAllCronJobs,
    ALL_CRON_JOBS_SWR_OPTIONS
  );

  const refetch = useCallback(async () => {
    await mutate();
  }, [mutate]);

  const eventHandlers = useMemo<CronJobEventHandlers>(
    () => ({
      onJobCreated: (job: ICronJob) => {
        void mutate((current) => upsertCronJobInList(current ?? [], job), { revalidate: false });
      },
      onJobUpdated: (job: ICronJob) => {
        void mutate((current) => upsertCronJobInList(current ?? [], job), { revalidate: false });
      },
      onJobRemoved: ({ cron_job_id }: { cron_job_id: CronJobId }) => {
        void mutate(
          (current) => (current ?? []).filter((item) => item.cron_job_id !== cron_job_id),
          { revalidate: false }
        );
      },
    }),
    [mutate]
  );

  useCronJobSubscription(eventHandlers, refetch);

  return {
    jobs: data ?? [],
    loading: isLoading,
    mutate,
    refetch,
  };
}

/**
 * Hook for managing all cron jobs across all conversations
 */
export function useAllCronJobs() {
  const { jobs, loading, mutate, refetch } = useAllCronJobsCache();

  const handleJobUpdated = useCallback(
    (cron_job_id: CronJobId, job: ICronJob) => {
      void mutate(
        (current) => (current ?? []).map((item) => (item.cron_job_id === cron_job_id ? job : item)),
        { revalidate: false }
      );
    },
    [mutate]
  );

  const handleJobDeleted = useCallback(
    (cron_job_id: CronJobId) => {
      void mutate(
        (current) => (current ?? []).filter((item) => item.cron_job_id !== cron_job_id),
        { revalidate: false }
      );
    },
    [mutate]
  );

  const actions = useCronJobActions(handleJobUpdated, handleJobDeleted);

  const activeCount = useMemo(() => jobs.filter((j) => j.enabled).length, [jobs]);
  const hasError = useMemo(() => jobs.some(isJobErrorLike), [jobs]);

  return {
    jobs,
    loading,
    activeCount,
    hasError,
    refetch,
    ...actions,
  };
}

/**
 * Hook for getting cron job status for all conversations
 * Used by ChatHistory to show indicators
 */
export function useCronJobsMap() {
  const { jobs, loading, refetch } = useAllCronJobsCache();
  const jobsMap = useMemo(() => indexCronJobsByConversation(jobs), [jobs]);
  const unreadStorageKey = browserStorageGenerationKey('cron-unread');
  // Track conversations with unread cron executions (red dot indicator)
  const [unreadConversations, setUnreadConversations] = useState<Set<ConversationId>>(() => {
    // Restore only from the current backend dataset generation. The old
    // unscoped key is intentionally not read after a hard database reset.
    try {
      const stored = localStorage.getItem(unreadStorageKey);
      if (stored) {
        const parsed = JSON.parse(stored);
        if (Array.isArray(parsed)) {
          const ids = parsed.flatMap((value) => {
            try {
              return [parseConversationId(value)];
            } catch {
              return [];
            }
          });
          return new Set(ids);
        }
      }
    } catch {
      // ignore
    }
    return new Set<ConversationId>();
  });
  // Track last_run_at_ms for each job to detect new executions
  const lastRunAtMapRef = useRef<Map<CronJobId, number>>(new Map());
  // Track current active conversation (use ref to access latest value in event handlers)
  const activeConversationIdRef = useRef<ConversationId | null>(null);

  // Persist unread state to localStorage
  useEffect(() => {
    try {
      localStorage.setItem(unreadStorageKey, JSON.stringify([...unreadConversations]));
    } catch {
      // ignore
    }
  }, [unreadConversations, unreadStorageKey]);

  useEffect(() => {
    for (const job of jobs) {
      if (job.state.last_run_at_ms && !lastRunAtMapRef.current.has(job.cron_job_id)) {
        lastRunAtMapRef.current.set(job.cron_job_id, job.state.last_run_at_ms);
      }
    }
  }, [jobs]);

  // Unread dots and sidebar sort are map-only side effects. Job rows themselves
  // come from the shared SWR snapshot (patched by useAllCronJobsCache).
  useEffect(() => {
    const unsubCreate = ipcBridge.cron.onJobCreated.on(() => {
      console.log('[useCronJobsMap] onJobCreated, triggering chat.history.refresh');
      emitter.emit('chat.history.refresh');
    });
    const unsubUpdate = ipcBridge.cron.onJobUpdated.on((job: ICronJob) => {
      const convId = job.metadata.conversation_id;
      const prevLastRunAt = lastRunAtMapRef.current.get(job.cron_job_id);
      const newLastRunAt = job.state.last_run_at_ms;
      if (convId && newLastRunAt && newLastRunAt !== prevLastRunAt) {
        lastRunAtMapRef.current.set(job.cron_job_id, newLastRunAt);

        if (activeConversationIdRef.current !== convId) {
          setUnreadConversations((prev) => {
            if (prev.has(convId)) return prev;
            const next = new Set(prev);
            next.add(convId);
            return next;
          });
        }

        emitter.emit('chat.history.refresh');
      }
    });
    return () => {
      unsubCreate();
      unsubUpdate();
    };
  }, []);

  const hasJobsForConversation = useCallback(
    (conversation_id: ConversationId) => {
      return jobsMap.has(conversation_id) && jobsMap.get(conversation_id)!.length > 0;
    },
    [jobsMap]
  );

  const getJobsForConversation = useCallback(
    (conversation_id: ConversationId): ICronJob[] => {
      return jobsMap.get(conversation_id) || [];
    },
    [jobsMap]
  );

  const getJobStatus = useCallback(
    (conversation_id: ConversationId): 'none' | 'active' | 'paused' | 'error' | 'unread' => {
      const convJobs = jobsMap.get(conversation_id);
      if (!convJobs || convJobs.length === 0) {
        return 'none';
      }

      if (unreadConversations.has(conversation_id)) return 'unread';

      if (convJobs.some(isJobErrorLike)) return 'error';

      if (convJobs.every((j) => !j.enabled)) return 'paused';

      return 'active';
    },
    [jobsMap, unreadConversations]
  );

  const markAsRead = useCallback((conversation_id: ConversationId) => {
    activeConversationIdRef.current = conversation_id;
    setUnreadConversations((prev) => {
      if (!prev.has(conversation_id)) {
        return prev;
      }
      const next = new Set(prev);
      next.delete(conversation_id);
      return next;
    });
  }, []);

  const setActiveConversation = useCallback((conversation_id: ConversationId) => {
    activeConversationIdRef.current = conversation_id;
  }, []);

  const hasUnread = useCallback(
    (conversation_id: ConversationId) => {
      return unreadConversations.has(conversation_id);
    },
    [unreadConversations]
  );

  return useMemo(
    () => ({
      jobsMap,
      loading,
      hasJobsForConversation,
      getJobsForConversation,
      getJobStatus,
      markAsRead,
      setActiveConversation,
      hasUnread,
      refetch,
    }),
    [
      jobsMap,
      loading,
      hasJobsForConversation,
      getJobsForConversation,
      getJobStatus,
      markAsRead,
      setActiveConversation,
      hasUnread,
      refetch,
    ]
  );
}

/**
 * Hook for fetching lightweight execution records for a specific cron job.
 * Each job is pruned server-side to its latest seven runs.
 */
export function useCronJobRuns(cron_job_id: CronJobId | undefined) {
  const [runs, setRuns] = useState<ICronJobRun[]>([]);
  const [loading, setLoading] = useState(false);

  const fetchRuns = useCallback(async () => {
    if (!cron_job_id) {
      setRuns([]);
      return;
    }

    setLoading(true);
    try {
      const result = await ipcBridge.cron.listRuns.invoke({ cron_job_id: cron_job_id });
      setRuns(result || []);
    } catch (err) {
      console.error('[useCronJobRuns] Failed to fetch:', err);
      setRuns([]);
    } finally {
      setLoading(false);
    }
  }, [cron_job_id]);

  // Initial fetch
  useEffect(() => {
    void fetchRuns();
  }, [fetchRuns]);

  // Refetch when this job executes. WebSocket delivery has no replay: also
  // reload the run history after any gap (reconnect, server lag resync).
  useEffect(() => {
    if (!cron_job_id) return;
    const unsubExecuted = ipcBridge.cron.onJobExecuted.on((data) => {
      if (data.cron_job_id === cron_job_id) {
        void fetchRuns();
      }
    });
    const unsubReconnected = ipcBridge.conversation.reconnected.on(() => void fetchRuns());
    return () => {
      unsubExecuted();
      unsubReconnected();
    };
  }, [cron_job_id, fetchRuns]);

  return { runs, loading };
}
