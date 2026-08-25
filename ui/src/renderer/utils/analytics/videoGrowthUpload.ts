import { httpRequest } from '@/common/adapter/httpBridge';
import type { FunnelEvent } from './productFunnel';

type GrowthProperty = string | number | boolean | null;

type VideoGrowthEvent = {
  eventId: string;
  name: string;
  occurredAt: string;
  properties: Record<string, GrowthProperty>;
  cohort?: 'A' | 'B';
};

type VideoGrowthUploadResponse = {
  accepted: number;
  duplicates: number;
};

export type VideoGrowthMetrics = {
  windowDays: number;
  wafc: number;
  ttfFilmP50Ms: number | null;
  ttfFilmP95Ms: number | null;
  startToFilmRate: number;
  filmSuccessRate: number;
  filmD7Rate: number;
  publishRate: number;
  generatedAt: string;
};

const QUEUE_KEY = 'flowy.growth.video.events.v1';
const MAX_QUEUE_SIZE = 500;
const BATCH_SIZE = 50;
const ALLOWED_PROPERTIES = new Set([
  'duration_secs',
  'error_code',
  'failure_channel',
  'has_references',
  'mode',
  'phase',
  'project_id',
  'runtime',
  'session_id',
  'source',
  'status',
  'viewport',
  'workflow',
]);

let memoryQueue: VideoGrowthEvent[] = [];
let cloudAuthenticated = false;
let flushPromise: Promise<void> | null = null;
let retryListenersInstalled = false;

function canUseStorage(): boolean {
  return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined';
}

function readQueue(): VideoGrowthEvent[] {
  if (!canUseStorage()) return memoryQueue;
  try {
    const parsed = JSON.parse(window.localStorage.getItem(QUEUE_KEY) ?? '[]') as VideoGrowthEvent[];
    return Array.isArray(parsed) ? parsed : memoryQueue;
  } catch {
    return memoryQueue;
  }
}

function writeQueue(events: VideoGrowthEvent[]): void {
  memoryQueue = events.slice(-MAX_QUEUE_SIZE);
  if (!canUseStorage()) return;
  try {
    window.localStorage.setItem(QUEUE_KEY, JSON.stringify(memoryQueue));
  } catch {
    // Keep the in-memory queue when storage is unavailable.
  }
}

function growthEventName(event: FunnelEvent): string | null {
  if (event.props?.feature !== 'video_generation') return null;
  if (event.name === 'first_value_confirmed') return null;
  return event.name;
}

function sanitizeProperties(
  properties: FunnelEvent['props']
): Record<string, GrowthProperty> {
  const sanitized: Record<string, GrowthProperty> = {};
  if (!properties) return sanitized;
  for (const [key, value] of Object.entries(properties)) {
    if (!ALLOWED_PROPERTIES.has(key)) continue;
    if (
      value === null ||
      typeof value === 'boolean' ||
      typeof value === 'number' ||
      typeof value === 'string'
    ) {
      sanitized[key] = typeof value === 'string' ? value.slice(0, 256) : value;
    }
  }
  return sanitized;
}

export function enqueueVideoGrowthEvent(event: FunnelEvent): void {
  const name = growthEventName(event);
  if (!name) return;
  const queue = readQueue();
  if (queue.some((queued) => queued.eventId === event.id)) return;
  queue.push({
    eventId: event.id,
    name,
    occurredAt: event.at,
    properties: sanitizeProperties(event.props),
    cohort: event.cohort,
  });
  writeQueue(queue);
  if (cloudAuthenticated) void flushVideoGrowthEvents();
}

export function flushVideoGrowthEvents(): Promise<void> {
  if (!cloudAuthenticated || flushPromise) return flushPromise ?? Promise.resolve();
  flushPromise = (async () => {
    while (cloudAuthenticated) {
      const batch = readQueue().slice(0, BATCH_SIZE);
      if (batch.length === 0) return;
      const response = await httpRequest<VideoGrowthUploadResponse>(
        'POST',
        '/api/cloud/growth/video/events',
        { events: batch },
        { silentStatuses: [401, 403] }
      );
      if (response.accepted + response.duplicates !== batch.length) {
        throw new Error('video growth upload acknowledgement mismatch');
      }
      const sentIds = new Set(batch.map((event) => event.eventId));
      writeQueue(readQueue().filter((event) => !sentIds.has(event.eventId)));
    }
  })()
    .catch(() => {
      // Retain the queue for the next authenticated or online retry.
    })
    .finally(() => {
      flushPromise = null;
    });
  return flushPromise;
}

export function setVideoGrowthCloudAuthenticated(authenticated: boolean): void {
  cloudAuthenticated = authenticated;
  if (!authenticated) return;
  if (!retryListenersInstalled && typeof window !== 'undefined') {
    retryListenersInstalled = true;
    window.addEventListener('online', () => void flushVideoGrowthEvents());
    document.addEventListener('visibilitychange', () => {
      if (!document.hidden) void flushVideoGrowthEvents();
    });
  }
  void flushVideoGrowthEvents();
}

export function getVideoGrowthMetrics(days = 7): Promise<VideoGrowthMetrics> {
  const boundedDays = Math.min(90, Math.max(1, Math.round(days)));
  return httpRequest<VideoGrowthMetrics>(
    'GET',
    `/api/cloud/growth/video/metrics?days=${boundedDays}`
  );
}

export function resetVideoGrowthUploadForTests(): void {
  memoryQueue = [];
  cloudAuthenticated = false;
  flushPromise = null;
  if (!canUseStorage()) return;
  try {
    window.localStorage.removeItem(QUEUE_KEY);
  } catch {
    // Ignore storage cleanup failures in tests.
  }
}

export function listQueuedVideoGrowthEventsForTests(): VideoGrowthEvent[] {
  return readQueue();
}
