import * as Sentry from '@sentry/react';
import posthog from 'posthog-js';
import { adoptBackendClientId, getInstallId } from './identity';

type ProductEventProps = Record<string, ProductEventProperty> | undefined;

const OPT_OUT_KEY = 'flowy.telemetry.optOut.v1';

export type ProductEventProperty = string | number | boolean | null;

const ALLOWED_PROPERTIES = new Set([
  'accept_ms',
  'blocker',
  'cohort',
  'cold_start',
  'conversation_type',
  'duration_secs',
  'error_code',
  'failure_channel',
  'feature',
  'finalization_gap_ms',
  'first_win_stage',
  'has_references',
  'hit_count',
  'intent',
  'item_type',
  'kind',
  'launchpad_variant',
  'method',
  'mode',
  'outcome',
  'phase',
  'project_id',
  'runtime',
  'session_id',
  'source',
  'status',
  'status_ms',
  'stream_ms',
  'total_ms',
  'ttft_ms',
  'viewport',
  'wait_ms',
  'workflow',
]);

let started = false;
let posthogReady = false;
let sentryReady = false;
let memoryOptOut: boolean | null = null;

function canUseStorage(): boolean {
  return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined';
}

function envString(value: unknown): string {
  return typeof value === 'string' ? value.trim() : '';
}

export function getPosthogKey(): string {
  return envString(import.meta.env.VITE_POSTHOG_KEY);
}

export function getPosthogHost(): string {
  return envString(import.meta.env.VITE_POSTHOG_HOST) || 'https://us.i.posthog.com';
}

export function getSentryDsn(): string {
  return envString(import.meta.env.VITE_SENTRY_DSN);
}

export function isTelemetryConfigured(): boolean {
  return Boolean(getPosthogKey() || getSentryDsn());
}

export function isTelemetryOptedOut(): boolean {
  if (memoryOptOut != null) return memoryOptOut;
  if (!canUseStorage()) return false;
  try {
    memoryOptOut = window.localStorage.getItem(OPT_OUT_KEY) === '1';
    return memoryOptOut;
  } catch {
    return false;
  }
}

export function isTelemetryEnabled(): boolean {
  return !isTelemetryOptedOut();
}

export function canSendThirdPartyTelemetry(): boolean {
  return isTelemetryConfigured() && isTelemetryEnabled();
}

export function setTelemetryOptOut(optOut: boolean): void {
  memoryOptOut = optOut;
  if (canUseStorage()) {
    try {
      if (optOut) window.localStorage.setItem(OPT_OUT_KEY, '1');
      else window.localStorage.removeItem(OPT_OUT_KEY);
    } catch {
      // ignore
    }
  }
  if (optOut) {
    shutdownTelemetry();
    return;
  }
  void startProductTelemetry();
}

export function sanitizeProductProperties(
  properties: ProductEventProps | Record<string, ProductEventProperty> | undefined
): Record<string, ProductEventProperty> {
  const sanitized: Record<string, ProductEventProperty> = {};
  if (!properties) return sanitized;
  for (const [key, value] of Object.entries(properties)) {
    if (!ALLOWED_PROPERTIES.has(key)) continue;
    if (value === null || typeof value === 'boolean' || typeof value === 'number') {
      sanitized[key] = value;
      continue;
    }
    if (typeof value === 'string') {
      sanitized[key] = value.slice(0, 256);
    }
  }
  return sanitized;
}

function shutdownTelemetry(): void {
  if (posthogReady) {
    posthog.opt_out_capturing();
    posthog.reset();
    posthogReady = false;
  }
  if (sentryReady) {
    Sentry.setUser(null);
    void Sentry.close(0);
    sentryReady = false;
  }
  started = false;
}

export async function startProductTelemetry(): Promise<void> {
  if (typeof window === 'undefined' || started || !canSendThirdPartyTelemetry()) return;
  started = true;
  const distinctId = getInstallId();

  const posthogKey = getPosthogKey();
  if (posthogKey) {
    posthog.init(posthogKey, {
      api_host: getPosthogHost(),
      person_profiles: 'identified_only',
      capture_pageview: false,
      capture_pageleave: false,
      disable_session_recording: true,
      mask_all_text: true,
      mask_all_element_attributes: true,
      persistence: 'localStorage',
      bootstrap: { distinctID: distinctId },
    });
    posthog.opt_in_capturing();
    posthog.identify(distinctId);
    posthogReady = true;
  }

  const sentryDsn = getSentryDsn();
  if (sentryDsn) {
    Sentry.init({
      dsn: sentryDsn,
      sendDefaultPii: false,
      tracesSampleRate: 0,
      replaysSessionSampleRate: 0,
      replaysOnErrorSampleRate: 0,
      beforeSend(event) {
        if (event.request?.headers) {
          delete event.request.headers.Authorization;
          delete event.request.headers.authorization;
          delete event.request.headers.Cookie;
          delete event.request.headers.cookie;
        }
        if (event.request?.cookies) {
          event.request.cookies = {};
        }
        return event;
      },
    });
    Sentry.setUser({ id: distinctId });
    sentryReady = true;
  }
}

export function captureProductEvent(
  name: string,
  properties?: ProductEventProps | Record<string, ProductEventProperty>
): void {
  if (!posthogReady || !canSendThirdPartyTelemetry()) return;
  posthog.capture(name, sanitizeProductProperties(properties));
}

export function captureFunnelEvent(event: {
  name: string;
  props?: ProductEventProps;
  cohort?: 'A' | 'B';
}): void {
  captureProductEvent(event.name, {
    ...event.props,
    cohort: event.cohort ?? null,
  });
}

export function identifyCloudUser(accountId: string): void {
  const trimmed = accountId.trim();
  if (!trimmed) return;
  const distinctId = getInstallId();
  if (posthogReady && canSendThirdPartyTelemetry()) {
    posthog.alias(trimmed, distinctId);
    posthog.identify(trimmed, { install_id: distinctId });
  }
  if (sentryReady && canSendThirdPartyTelemetry()) {
    Sentry.setUser({ id: trimmed });
  }
}

export function resetCloudUserIdentity(): void {
  const distinctId = getInstallId();
  if (posthogReady && isTelemetryEnabled()) {
    posthog.reset();
    posthog.identify(distinctId);
  }
  if (sentryReady) {
    Sentry.setUser({ id: distinctId });
  }
}

export function syncBackendClientId(clientId: string | undefined | null): string {
  if (!clientId?.trim()) return getInstallId();
  const next = adoptBackendClientId(clientId);
  if (posthogReady && canSendThirdPartyTelemetry()) {
    posthog.identify(next);
  }
  if (sentryReady && canSendThirdPartyTelemetry()) {
    Sentry.setUser({ id: next });
  }
  return next;
}

export function captureException(error: unknown): void {
  if (!sentryReady || !canSendThirdPartyTelemetry()) return;
  Sentry.captureException(error);
}

export function resetTelemetryForTests(): void {
  shutdownTelemetry();
  memoryOptOut = null;
  if (canUseStorage()) {
    try {
      window.localStorage.removeItem(OPT_OUT_KEY);
    } catch {
      // ignore
    }
  }
}
