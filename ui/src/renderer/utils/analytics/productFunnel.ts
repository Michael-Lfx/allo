/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export type FunnelEventName =
  | 'auth_completed'
  | 'home_interactive'
  | 'task_drafted'
  | 'prerequisite_resolved'
  | 'task_accepted'
  | 'first_task_started'
  | 'first_artifact_visible'
  | 'answer_completed'
  | 'first_value_confirmed'
  | 'd1_retained'
  | 'd7_retained'
  | 'message_submitted'
  | 'message_accepted'
  | 'first_status'
  | 'first_token'
  | 'stream_finished'
  | 'turn_idle'
  | 'retry_succeeded'
  | 'abandoned_before_first_token';

export type FunnelEvent = {
  name: FunnelEventName;
  at: string;
  props?: Record<string, string | number | boolean | null>;
  cohort?: 'A' | 'B';
};

const STORAGE_KEY = 'flowy.funnel.events.v1';
const COHORT_KEY = 'flowy.funnel.cohort.v1';

let memoryEvents: FunnelEvent[] = [];
let memoryCohort: 'A' | 'B' | null = null;

function canUseStorage(): boolean {
  return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined';
}

function readEvents(): FunnelEvent[] {
  if (!canUseStorage()) return memoryEvents;
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return memoryEvents;
    const parsed = JSON.parse(raw) as FunnelEvent[];
    return Array.isArray(parsed) ? parsed : memoryEvents;
  } catch {
    return memoryEvents;
  }
}

function writeEvents(events: FunnelEvent[]): void {
  memoryEvents = events.slice(-200);
  if (!canUseStorage()) return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(memoryEvents));
  } catch {
    // ignore
  }
}

export function getFunnelCohort(): 'A' | 'B' {
  if (memoryCohort) return memoryCohort;
  if (canUseStorage()) {
    try {
      const existing = window.localStorage.getItem(COHORT_KEY);
      if (existing === 'A' || existing === 'B') {
        memoryCohort = existing;
        return existing;
      }
      const next = Math.random() < 0.5 ? 'A' : 'B';
      window.localStorage.setItem(COHORT_KEY, next);
      memoryCohort = next;
      return next;
    } catch {
      // fall through
    }
  }
  memoryCohort = Math.random() < 0.5 ? 'A' : 'B';
  return memoryCohort;
}

/** Stable A/B stratification dims for login→first-task analysis. */
export function getFunnelSegmentProps(): FunnelEvent['props'] {
  const desktopShell =
    typeof window !== 'undefined' && Boolean((window as { __backendPort?: number }).__backendPort);
  const mobileViewport =
    typeof window !== 'undefined' &&
    typeof window.matchMedia === 'function' &&
    window.matchMedia('(max-width: 768px)').matches;
  let firstWinCompleted = false;
  let launchpadVariant: string | null = null;
  if (canUseStorage()) {
    try {
      firstWinCompleted = window.localStorage.getItem('flowy.firstWin.completed.v1') === '1';
      const stored = window.localStorage.getItem('flowy.experiment.launchpad.v1');
      if (stored === 'control' || stored === 'launchpad') {
        launchpadVariant = stored;
      } else {
        // Mirror launchpadExperiment assignment without importing it (cycle-safe).
        launchpadVariant = getFunnelCohort() === 'A' ? 'launchpad' : 'control';
        window.localStorage.setItem('flowy.experiment.launchpad.v1', launchpadVariant);
      }
    } catch {
      // ignore
    }
  }
  return {
    runtime: desktopShell ? 'desktop' : 'webui',
    viewport: mobileViewport ? 'mobile' : 'desktop',
    first_win_stage: firstWinCompleted ? 'completed' : 'active',
    launchpad_variant: launchpadVariant,
  };
}

export function trackFunnelEvent(name: FunnelEventName, props?: FunnelEvent['props']): FunnelEvent {
  const event: FunnelEvent = {
    name,
    at: new Date().toISOString(),
    props: {
      ...getFunnelSegmentProps(),
      ...props,
    },
    cohort: getFunnelCohort(),
  };
  const events = readEvents();
  events.push(event);
  writeEvents(events);
  if (typeof window !== 'undefined' && typeof window.dispatchEvent === 'function') {
    window.dispatchEvent(new CustomEvent('flowy:funnel', { detail: event }));
  }
  return event;
}

export function listFunnelEvents(): FunnelEvent[] {
  return readEvents();
}

export function hasFunnelEvent(name: FunnelEventName): boolean {
  return readEvents().some((event) => event.name === name);
}

export function resetFunnelForTests(): void {
  memoryEvents = [];
  memoryCohort = null;
  if (!canUseStorage()) return;
  try {
    window.localStorage.removeItem(STORAGE_KEY);
    window.localStorage.removeItem(COHORT_KEY);
  } catch {
    // ignore
  }
}

/** Mark D1/D7 if first auth is old enough and user returns. */
export function maybeTrackRetention(now = Date.now()): FunnelEvent[] {
  const auth = readEvents().find((event) => event.name === 'auth_completed');
  if (!auth) return [];
  const authAt = Date.parse(auth.at);
  if (!Number.isFinite(authAt)) return [];
  const dayMs = 24 * 60 * 60 * 1000;
  const emitted: FunnelEvent[] = [];
  if (now - authAt >= dayMs && !hasFunnelEvent('d1_retained')) {
    emitted.push(trackFunnelEvent('d1_retained'));
  }
  if (now - authAt >= 7 * dayMs && !hasFunnelEvent('d7_retained')) {
    emitted.push(trackFunnelEvent('d7_retained'));
  }
  return emitted;
}

/**
 * User-confirmed first value (open artifact / copy / follow-up / explicit confirm).
 * First token alone is never enough.
 */
export function confirmFirstValue(props?: FunnelEvent['props']): FunnelEvent | null {
  if (hasFunnelEvent('first_value_confirmed')) return null;
  return trackFunnelEvent('first_value_confirmed', props);
}

export type TurnTimingProps = {
  conversation_type?: string;
  cold_start?: boolean;
  error_code?: string | null;
};

type TurnTimingSession = {
  requestKey: string;
  submittedAt: number;
  acceptedAt?: number;
  firstStatusAt?: number;
  firstTokenAt?: number;
  streamFinishedAt?: number;
  props: TurnTimingProps;
};

const turnTimingSessions = new Map<string, TurnTimingSession>();

export function beginTurnTiming(requestKey: string, props: TurnTimingProps = {}): void {
  const submittedAt = Date.now();
  turnTimingSessions.set(requestKey, { requestKey, submittedAt, props });
  trackFunnelEvent('message_submitted', {
    request_key: requestKey,
    conversation_type: props.conversation_type ?? null,
    cold_start: props.cold_start ?? null,
  });
}

export function markTurnAccepted(requestKey: string, extra?: TurnTimingProps): number | null {
  const session = turnTimingSessions.get(requestKey);
  if (!session || session.acceptedAt != null) return null;
  session.acceptedAt = Date.now();
  Object.assign(session.props, extra);
  const acceptMs = session.acceptedAt - session.submittedAt;
  trackFunnelEvent('message_accepted', {
    request_key: requestKey,
    accept_ms: acceptMs,
    conversation_type: session.props.conversation_type ?? null,
    cold_start: session.props.cold_start ?? null,
  });
  return acceptMs;
}

export function markTurnFirstStatus(requestKey: string, phase?: string): number | null {
  const session = turnTimingSessions.get(requestKey);
  if (!session || session.firstStatusAt != null) return null;
  session.firstStatusAt = Date.now();
  const statusMs = session.firstStatusAt - session.submittedAt;
  trackFunnelEvent('first_status', {
    request_key: requestKey,
    status_ms: statusMs,
    phase: phase ?? null,
    conversation_type: session.props.conversation_type ?? null,
  });
  return statusMs;
}

export function markTurnFirstToken(requestKey: string): number | null {
  const session = turnTimingSessions.get(requestKey);
  if (!session || session.firstTokenAt != null) return null;
  session.firstTokenAt = Date.now();
  const ttftMs = session.firstTokenAt - session.submittedAt;
  trackFunnelEvent('first_token', {
    request_key: requestKey,
    ttft_ms: ttftMs,
    conversation_type: session.props.conversation_type ?? null,
    cold_start: session.props.cold_start ?? null,
  });
  return ttftMs;
}

export function markTurnStreamFinished(requestKey: string): number | null {
  const session = turnTimingSessions.get(requestKey);
  if (!session || session.streamFinishedAt != null) return null;
  session.streamFinishedAt = Date.now();
  const streamMs = session.streamFinishedAt - session.submittedAt;
  trackFunnelEvent('stream_finished', {
    request_key: requestKey,
    stream_ms: streamMs,
    conversation_type: session.props.conversation_type ?? null,
  });
  return streamMs;
}

export function markTurnIdle(requestKey: string, outcome: 'completed' | 'failed' | 'cancelled' = 'completed'): number | null {
  const session = turnTimingSessions.get(requestKey);
  if (!session) return null;
  const idleAt = Date.now();
  const finalizationGapMs =
    session.streamFinishedAt != null ? idleAt - session.streamFinishedAt : null;
  const totalMs = idleAt - session.submittedAt;
  trackFunnelEvent('turn_idle', {
    request_key: requestKey,
    total_ms: totalMs,
    finalization_gap_ms: finalizationGapMs,
    outcome,
    conversation_type: session.props.conversation_type ?? null,
    cold_start: session.props.cold_start ?? null,
    error_code: session.props.error_code ?? null,
  });
  if (outcome === 'completed') {
    if (!hasFunnelEvent('answer_completed')) {
      trackFunnelEvent('answer_completed', {
        request_key: requestKey,
        conversation_type: session.props.conversation_type ?? null,
      });
    }
    if (!hasFunnelEvent('first_artifact_visible')) {
      trackFunnelEvent('first_artifact_visible', {
        request_key: requestKey,
        source: 'answer',
        conversation_type: session.props.conversation_type ?? null,
      });
    }
  }
  turnTimingSessions.delete(requestKey);
  return totalMs;
}

export function markTurnAbandonedBeforeFirstToken(requestKey: string): void {
  const session = turnTimingSessions.get(requestKey);
  if (!session || session.firstTokenAt != null) return;
  trackFunnelEvent('abandoned_before_first_token', {
    request_key: requestKey,
    wait_ms: Date.now() - session.submittedAt,
    conversation_type: session.props.conversation_type ?? null,
  });
  turnTimingSessions.delete(requestKey);
}

export function markRetrySucceeded(requestKey: string, props?: TurnTimingProps): void {
  trackFunnelEvent('retry_succeeded', {
    request_key: requestKey,
    conversation_type: props?.conversation_type ?? null,
    error_code: props?.error_code ?? null,
  });
}

export function clearTurnTiming(requestKey: string): void {
  turnTimingSessions.delete(requestKey);
}

export function resetTurnTimingForTests(): void {
  turnTimingSessions.clear();
}
