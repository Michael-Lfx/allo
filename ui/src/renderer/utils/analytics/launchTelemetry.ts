import { scheduleDeferred } from '@/renderer/utils/scheduleDeferred';
import {
  trackFunnelEvent,
  trackFunnelEventOnce,
  type FunnelEvent,
  type FunnelEventName,
} from './productFunnel';

type LaunchTelemetryProps = NonNullable<FunnelEvent['props']>;

const SESSION_BOOT_KEY = 'flowy.app_launch.boot.v1';

type LaunchSession = {
  bootAt: number;
  coldStart: boolean;
  authReadyAt?: number;
  configReadyAt?: number;
  interactiveAt?: number;
  completed: boolean;
  failed: boolean;
};

type PendingLaunchEvent = {
  name: FunnelEventName;
  props: LaunchTelemetryProps;
  eventId?: string;
};

let session: LaunchSession | null = null;
let pendingEvents: PendingLaunchEvent[] = [];
let cancelDeferredFlush: (() => void) | null = null;

function nowMs(): number {
  return typeof performance !== 'undefined' && typeof performance.now === 'function'
    ? performance.now()
    : Date.now();
}

function canUseSessionStorage(): boolean {
  return typeof window !== 'undefined' && typeof window.sessionStorage !== 'undefined';
}

function resolveColdStart(): boolean {
  if (!canUseSessionStorage()) return true;
  try {
    if (window.sessionStorage.getItem(SESSION_BOOT_KEY) === '1') return false;
    window.sessionStorage.setItem(SESSION_BOOT_KEY, '1');
    return true;
  } catch {
    return true;
  }
}

function baseProps(extra?: LaunchTelemetryProps): LaunchTelemetryProps {
  return {
    feature: 'app_launch',
    cold_start: session?.coldStart ?? null,
    ...extra,
  };
}

function ensureSession(): LaunchSession {
  if (session) return session;
  session = {
    bootAt: nowMs(),
    coldStart: resolveColdStart(),
    completed: false,
    failed: false,
  };
  return session;
}

function drainPendingLaunchEvents(): void {
  const batch = pendingEvents;
  pendingEvents = [];
  for (const event of batch) {
    if (event.eventId) {
      trackFunnelEventOnce(event.name, event.eventId, event.props);
    } else {
      trackFunnelEvent(event.name, event.props);
    }
  }
}

/**
 * Persist/report off the launch critical path. Timestamps are captured at mark
 * time; localStorage + outbox + HTTP only run after first paint settles.
 */
function scheduleLaunchTelemetryFlush(): void {
  if (cancelDeferredFlush) return;
  if (typeof window === 'undefined') {
    // Unit tests drain explicitly via flushLaunchTelemetryForTests().
    return;
  }
  cancelDeferredFlush = scheduleDeferred(() => {
    cancelDeferredFlush = null;
    drainPendingLaunchEvents();
  });
}

function enqueueLaunchEvent(
  name: FunnelEventName,
  props: LaunchTelemetryProps,
  eventId?: string
): void {
  pendingEvents.push({ name, props, eventId });
  scheduleLaunchTelemetryFlush();
}

/** Call as early as practical in the renderer entry (before React render). */
export function markLaunchBootStarted(): void {
  ensureSession();
}

/** Sync mark only — no funnel/outbox/storage writes on the hot path. */
export function markLaunchAuthReady(props?: {
  status?: 'authenticated' | 'unauthenticated' | 'unknown';
}): boolean {
  const current = ensureSession();
  if (current.authReadyAt != null) return false;
  current.authReadyAt = nowMs();
  const durationMs = Math.max(0, Math.round(current.authReadyAt - current.bootAt));
  enqueueLaunchEvent(
    'app_launch_auth_ready',
    baseProps({
      phase: 'auth',
      duration_ms: durationMs,
      status: props?.status ?? 'unknown',
    })
  );
  return true;
}

export function markLaunchConfigReady(): boolean {
  const current = ensureSession();
  if (current.configReadyAt != null || current.failed || current.completed) return false;
  current.configReadyAt = nowMs();
  const durationMs = Math.max(0, Math.round(current.configReadyAt - current.bootAt));
  const waitMs =
    current.authReadyAt != null
      ? Math.max(0, Math.round(current.configReadyAt - current.authReadyAt))
      : null;
  enqueueLaunchEvent(
    'app_launch_config_ready',
    baseProps({
      phase: 'config',
      duration_ms: durationMs,
      wait_ms: waitMs,
      status: 'ready',
    })
  );
  return true;
}

/**
 * First paint / shell usable. Prefer calling after configReady when authenticated,
 * or when the login route is shown for unauthenticated sessions.
 */
export function markLaunchInteractive(props?: {
  source?: string | null;
}): boolean {
  const current = ensureSession();
  if (current.interactiveAt != null || current.failed) return false;
  current.interactiveAt = nowMs();
  const durationMs = Math.max(0, Math.round(current.interactiveAt - current.bootAt));
  const waitMs =
    current.configReadyAt != null
      ? Math.max(0, Math.round(current.interactiveAt - current.configReadyAt))
      : current.authReadyAt != null
        ? Math.max(0, Math.round(current.interactiveAt - current.authReadyAt))
        : null;
  enqueueLaunchEvent(
    'app_launch_interactive',
    baseProps({
      phase: 'interactive',
      duration_ms: durationMs,
      wait_ms: waitMs,
      source: props?.source ?? null,
      status: 'interactive',
    })
  );
  maybeCompleteLaunch('succeeded', props?.source ?? null);
  return true;
}

export function markLaunchFailed(props: {
  error_code?: string | null;
  blocker?: string | null;
  phase?: string | null;
}): boolean {
  const current = ensureSession();
  if (current.failed || current.completed) return false;
  current.failed = true;
  const durationMs = Math.max(0, Math.round(nowMs() - current.bootAt));
  enqueueLaunchEvent(
    'app_launch_failed',
    baseProps({
      phase: props.phase ?? 'config',
      duration_ms: durationMs,
      total_ms: durationMs,
      status: 'failed',
      outcome: 'failed',
      error_code: props.error_code ?? 'startup_failed',
      blocker: props.blocker ?? null,
    })
  );
  return true;
}

function maybeCompleteLaunch(
  outcome: 'succeeded' | 'degraded',
  source: string | null
): void {
  const current = ensureSession();
  if (current.completed || current.failed || current.interactiveAt == null) return;
  current.completed = true;
  const totalMs = Math.max(0, Math.round(current.interactiveAt - current.bootAt));
  const eventId = `app_launch:completed:${current.coldStart ? 'cold' : 'warm'}:${Math.round(current.bootAt)}`;
  enqueueLaunchEvent(
    'app_launch_completed',
    baseProps({
      phase: 'completed',
      duration_ms: totalMs,
      total_ms: totalMs,
      status: outcome,
      outcome,
      source,
      already_ready: !current.coldStart,
    }),
    eventId
  );
}

/** Test helper: drain deferred emits immediately (production uses scheduleDeferred). */
export function flushLaunchTelemetryForTests(): void {
  cancelDeferredFlush?.();
  cancelDeferredFlush = null;
  drainPendingLaunchEvents();
}

export function pendingLaunchTelemetryCountForTests(): number {
  return pendingEvents.length;
}

export function resetLaunchTelemetryForTests(): void {
  cancelDeferredFlush?.();
  cancelDeferredFlush = null;
  pendingEvents = [];
  session = null;
  if (!canUseSessionStorage()) return;
  try {
    window.sessionStorage.removeItem(SESSION_BOOT_KEY);
  } catch {
    // ignore
  }
}
