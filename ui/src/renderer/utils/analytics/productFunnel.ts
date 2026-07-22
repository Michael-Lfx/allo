/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export type FunnelEventName =
  | 'auth_completed'
  | 'first_task_started'
  | 'first_value_confirmed'
  | 'd1_retained'
  | 'd7_retained';

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

export function trackFunnelEvent(name: FunnelEventName, props?: FunnelEvent['props']): FunnelEvent {
  const event: FunnelEvent = {
    name,
    at: new Date().toISOString(),
    props,
    cohort: getFunnelCohort(),
  };
  const events = readEvents();
  events.push(event);
  writeEvents(events);
  if (typeof window !== 'undefined') {
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
