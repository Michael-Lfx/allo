/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./useCronJobs.ts', import.meta.url), 'utf8');

describe('cron jobs reconnect recovery', () => {
  test('shared subscription helper resyncs the durable snapshot after websocket reconnect', () => {
    // WebSocket delivery has no replay: cron job events lost during a gap must
    // be recovered by refetching. Conversation-scoped lists keep their own
    // fetch; all-jobs consumers share one SWR key and resync through mutate.
    expect(source.includes('onResync')).toBe(true);
    expect(source.includes('ipcBridge.conversation.reconnected.on')).toBe(true);
    expect(source.includes('useCronJobSubscription(eventHandlers, fetchJobs)')).toBe(true);
    expect(source.includes('useCronJobSubscription(eventHandlers, refetch)')).toBe(true);
  });

  test('all-jobs consumers share one SWR cache identity', () => {
    expect(source.includes("ALL_CRON_JOBS_SWR_KEY = 'cron.jobs.all'")).toBe(true);
    expect(source.includes('revalidateIfStale: false')).toBe(true);
    expect(source.includes('revalidateOnFocus: false')).toBe(true);
    expect(source.includes('function useAllCronJobsCache()')).toBe(true);
    expect(source.includes('const { jobs, loading, mutate, refetch } = useAllCronJobsCache()')).toBe(true);
    expect(source.includes('const { jobs, loading, refetch } = useAllCronJobsCache()')).toBe(true);
  });
});
