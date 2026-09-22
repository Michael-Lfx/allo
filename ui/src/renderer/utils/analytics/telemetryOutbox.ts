import { httpRequest } from '@/common/adapter/httpBridge';
import { isTelemetryEnabled } from './telemetry';
import type { FunnelEvent } from './productFunnel';

type TelemetryProperty = string | number | boolean | null;

export type FirstPartyTelemetryEvent = {
  eventId: string;
  name: string;
  occurredAt: string;
  module: 'video_generation' | 'platform' | 'commerce' | 'conversation' | 'knowledge';
  properties: Record<string, TelemetryProperty>;
  cohort?: 'A' | 'B';
};

type TelemetryUploadResponse = {
  accepted: number;
  duplicates: number;
  rejected?: number;
};

const QUEUE_KEY = 'flowy.telemetry.events.v1';
const LEGACY_QUEUE_KEY = 'flowy.growth.video.events.v1';
const MAX_QUEUE_SIZE = 500;
const BATCH_SIZE = 50;
const ALLOWED_PROPERTIES = new Set([
  'accept_ms',
  'balance',
  'already_ready',
  'amount',
  'average_bps',
  'blocker',
  'bytes_total',
  'bytes_transferred',
  'cdn_host',
  'cold_start',
  'conversation_type',
  'coupon',
  'credits_consumed',
  'currency',
  'duration_ms',
  'duration_secs',
  'error_code',
  'error_message',
  'failure_channel',
  'feature',
  'finalization_gap_ms',
  'from_version',
  'has_references',
  'hit_count',
  'image_model',
  'kind',
  'launchpad_variant',
  'llm_model',
  'locale',
  'mode',
  'network_class',
  'order_no',
  'outcome',
  'payment_channel',
  'peak_bps',
  'phase',
  'plan_id',
  'playbook_id',
  'project_id',
  'request_key',
  'runtime',
  'session_id',
  'source',
  'status',
  'status_ms',
  'stream_ms',
  'threshold',
  'to_version',
  'total_ms',
  'ttft_ms',
  'tz_offset_min',
  'video_model',
  'viewport',
  'wait_ms',
  'workflow',
  'briefing_id',
  'research_depth',
  'beat_count',
  'citation_count',
  'market_source',
  'package_slug',
  'failure_class',
  'failure_code',
  'skill_slug',
  'http_status',
]);

const PLATFORM_EVENT_NAMES = new Set([
  'app_opened',
  'app_launch_auth_ready',
  'app_launch_config_ready',
  'app_launch_interactive',
  'app_launch_failed',
  'app_launch_completed',
  'auth_completed',
  'home_interactive',
  'expert_package_install_failed',
  'd1_retained',
  'd7_retained',
  'device_activated',
  'experiment_exposed',
  'provider_degraded',
  'update_check_completed',
  'update_prompt_shown',
  'update_download_started',
  'update_download_succeeded',
  'update_download_failed',
  'update_install_started',
  'update_install_failed',
  'update_install_blocked',
  'update_applied',
]);

const COMMERCE_EVENT_NAMES = new Set([
  'billing_catalog_viewed',
  'billing_catalog_open_failed',
  'billing_checkout_started',
  'billing_pay_started',
  'billing_pay_succeeded',
  'billing_pay_failed',
  'low_credit_balance',
]);

const CONVERSATION_EVENT_NAMES = new Set([
  'message_submitted',
  'message_accepted',
  'first_status',
  'first_token',
  'stream_finished',
  'turn_idle',
  'retry_succeeded',
  'abandoned_before_first_token',
  'answer_completed',
  'llm_call_failed',
]);

const KNOWLEDGE_EVENT_NAMES = new Set(['kb_created', 'kb_grounded']);

const FEATURE_SCOPED_EVENT_NAMES = new Set([
  'task_drafted',
  'task_accepted',
  'first_task_started',
  'first_artifact_visible',
  'prerequisite_resolved',
  'value_confirmed',
  'first_value_confirmed',
]);

const VIDEO_EVENT_NAMES = new Set([
  'render_started',
  'film_succeeded',
  'film_failed',
  'film_cancelled',
  'briefing_succeeded',
  'briefing_failed',
  'briefing_cancelled',
  'project_exported',
  'tv_published',
  'resume_started',
  'resume_succeeded',
]);

let memoryQueue: FirstPartyTelemetryEvent[] = [];
let cloudAuthenticated = false;
let flushPromise: Promise<void> | null = null;
let retryListenersInstalled = false;
let lastFailureAt = 0;
const MIN_RETRY_INTERVAL_MS = 60_000;

function canUseStorage(): boolean {
  return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined';
}

function parseQueue(raw: string | null): FirstPartyTelemetryEvent[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw) as FirstPartyTelemetryEvent[];
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function migrateLegacyEvent(event: FirstPartyTelemetryEvent): FirstPartyTelemetryEvent {
  return {
    ...event,
    module: event.module === 'platform' ? 'platform' : 'video_generation',
  };
}

function readQueue(): FirstPartyTelemetryEvent[] {
  if (!canUseStorage()) return memoryQueue;
  try {
    const current = parseQueue(window.localStorage.getItem(QUEUE_KEY));
    if (current.length > 0) {
      memoryQueue = current;
      return current;
    }
    const legacy = parseQueue(window.localStorage.getItem(LEGACY_QUEUE_KEY)).map(migrateLegacyEvent);
    if (legacy.length > 0) {
      writeQueue(legacy);
      window.localStorage.removeItem(LEGACY_QUEUE_KEY);
      return memoryQueue;
    }
    return memoryQueue;
  } catch {
    return memoryQueue;
  }
}

function writeQueue(events: FirstPartyTelemetryEvent[]): void {
  memoryQueue = events.slice(-MAX_QUEUE_SIZE);
  if (!canUseStorage()) return;
  try {
    window.localStorage.setItem(QUEUE_KEY, JSON.stringify(memoryQueue));
  } catch {
    // Keep the in-memory queue when storage is unavailable.
  }
}

function moduleForFeature(feature: unknown): FirstPartyTelemetryEvent['module'] {
  if (feature === 'video_generation') return 'video_generation';
  if (feature === 'knowledge') return 'knowledge';
  return 'conversation';
}

function firstPartyModule(event: FunnelEvent): FirstPartyTelemetryEvent['module'] | null {
  if (PLATFORM_EVENT_NAMES.has(event.name)) return 'platform';
  if (COMMERCE_EVENT_NAMES.has(event.name)) return 'commerce';
  if (KNOWLEDGE_EVENT_NAMES.has(event.name)) return 'knowledge';
  if (CONVERSATION_EVENT_NAMES.has(event.name)) return 'conversation';
  if (event.name === 'home_viewed') {
    return event.props?.feature === 'video_generation' ? 'video_generation' : 'platform';
  }
  if (FEATURE_SCOPED_EVENT_NAMES.has(event.name)) return moduleForFeature(event.props?.feature);
  if (VIDEO_EVENT_NAMES.has(event.name)) return 'video_generation';
  return null;
}

function sanitizeProperties(
  properties: FunnelEvent['props']
): Record<string, TelemetryProperty> {
  const sanitized: Record<string, TelemetryProperty> = {};
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

export function enqueueTelemetryEvent(event: FunnelEvent): void {
  if (!isTelemetryEnabled()) return;
  const module = firstPartyModule(event);
  if (!module) return;
  const queue = readQueue();
  if (queue.some((queued) => queued.eventId === event.id)) return;
  queue.push({
    eventId: event.id,
    name: event.name,
    occurredAt: event.at,
    module,
    properties: sanitizeProperties(event.props),
    cohort: event.cohort,
  });
  writeQueue(queue);
  if (cloudAuthenticated) void flushTelemetryEvents();
}

export function flushTelemetryEvents(): Promise<void> {
  if (!cloudAuthenticated || flushPromise) return flushPromise ?? Promise.resolve();
  if (lastFailureAt && Date.now() - lastFailureAt < MIN_RETRY_INTERVAL_MS) {
    return Promise.resolve();
  }
  flushPromise = (async () => {
    while (cloudAuthenticated) {
      const batch = readQueue().slice(0, BATCH_SIZE);
      if (batch.length === 0) {
        lastFailureAt = 0;
        return;
      }
      const response = await httpRequest<TelemetryUploadResponse>(
        'POST',
        '/api/cloud/telemetry/events',
        { events: batch },
        { silentStatuses: [400, 401, 403, 404, 429, 502, 503, 504] }
      ).catch(() => {
        lastFailureAt = Date.now();
        return null;
      });
      if (!response) {
        lastFailureAt = Date.now();
        return;
      }
      const rejected = response.rejected ?? 0;
      if (response.accepted + response.duplicates + rejected !== batch.length) {
        throw new Error('telemetry upload acknowledgement mismatch');
      }
      const sentIds = new Set(batch.map((event) => event.eventId));
      writeQueue(readQueue().filter((event) => !sentIds.has(event.eventId)));
      lastFailureAt = 0;
    }
  })()
    .catch(() => {
      lastFailureAt = Date.now();
    })
    .finally(() => {
      flushPromise = null;
    });
  return flushPromise;
}

export function setTelemetryCloudAuthenticated(authenticated: boolean): void {
  cloudAuthenticated = authenticated;
  if (!authenticated) return;
  if (!retryListenersInstalled && typeof window !== 'undefined') {
    retryListenersInstalled = true;
    window.addEventListener('online', () => void flushTelemetryEvents());
    document.addEventListener('visibilitychange', () => {
      if (!document.hidden) void flushTelemetryEvents();
    });
  }
  void flushTelemetryEvents();
}

export function resetTelemetryOutboxForTests(): void {
  memoryQueue = [];
  cloudAuthenticated = false;
  flushPromise = null;
  lastFailureAt = 0;
  if (!canUseStorage()) return;
  try {
    window.localStorage.removeItem(QUEUE_KEY);
    window.localStorage.removeItem(LEGACY_QUEUE_KEY);
  } catch {
    // Ignore storage cleanup failures in tests.
  }
}

export function listQueuedTelemetryEventsForTests(): FirstPartyTelemetryEvent[] {
  return readQueue();
}

/** @deprecated Use enqueueTelemetryEvent. */
export const enqueueVideoGrowthEvent = enqueueTelemetryEvent;
/** @deprecated Use flushTelemetryEvents. */
export const flushVideoGrowthEvents = flushTelemetryEvents;
/** @deprecated Use setTelemetryCloudAuthenticated. */
export const setVideoGrowthCloudAuthenticated = setTelemetryCloudAuthenticated;
/** @deprecated Use resetTelemetryOutboxForTests. */
export const resetVideoGrowthUploadForTests = resetTelemetryOutboxForTests;
/** @deprecated Use listQueuedTelemetryEventsForTests. */
export const listQueuedVideoGrowthEventsForTests = listQueuedTelemetryEventsForTests;
