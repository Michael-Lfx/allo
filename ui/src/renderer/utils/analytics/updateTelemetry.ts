import { isDesktopShell } from '@/renderer/utils/platform';
import { tauriUpdateCurrentVersion } from '@/common/adapter/tauriUpdater';
import {
  AUTO_INSTALL_UNSUPPORTED_ERROR,
  INSTALL_NOT_ATTEMPTED_ERROR,
} from '@/common/adapter/tauriUpdateInstall';
import { trackFunnelEvent, trackFunnelEventOnce, type FunnelEvent } from './productFunnel';

export type UpdateTelemetrySource =
  | 'startup'
  | 'titlebar'
  | 'about'
  | 'menu'
  | 'modal'
  | 'unknown';

export type UpdateCheckStatus = 'available' | 'up_to_date' | 'failed';

type UpdateTelemetryProps = NonNullable<FunnelEvent['props']>;

const PENDING_APPLY_KEY = 'flowy.update.pending_apply.v1';

type PendingApply = {
  fromVersion: string;
  toVersion: string;
  markedAt: string;
};

function canUseStorage(): boolean {
  return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined';
}

function normalizeVersion(version?: string | null): string | null {
  if (!version) return null;
  const trimmed = version.trim();
  if (!trimmed) return null;
  return trimmed.startsWith('v') ? trimmed.slice(1) : trimmed;
}

/** ModelScope is the only OTA origin today; keep host explicit for CDN iteration. */
export const UPDATE_CDN_HOST = 'modelscope.cn';

/** Below this sustained rate, mark download/check as slow for growth dashboards. */
export const SLOW_DOWNLOAD_BPS = 256_000; // 250 KiB/s
export const FAST_DOWNLOAD_BPS = 1_500_000; // ~1.5 MiB/s
export const SLOW_CHECK_MS = 8_000;

export type UpdateNetworkClass = 'fast' | 'ok' | 'slow' | 'failed' | 'cache' | 'unknown';

function clientLocale(): string | null {
  if (typeof navigator === 'undefined') return null;
  const language = navigator.language?.trim();
  return language ? language.slice(0, 32) : null;
}

/** Minutes east of UTC (Asia/Shanghai = 480). */
function clientTimezoneOffsetMinutes(): number | null {
  try {
    return -new Date().getTimezoneOffset();
  } catch {
    return null;
  }
}

function nonNegInt(value: number | null | undefined): number | null {
  if (typeof value !== 'number' || !Number.isFinite(value)) return null;
  return Math.max(0, Math.round(value));
}

export function computeAverageBps(
  bytes: number | null | undefined,
  durationMs: number | null | undefined
): number | null {
  const size = nonNegInt(bytes);
  const ms = nonNegInt(durationMs);
  if (size == null || ms == null || ms <= 0 || size <= 0) return null;
  return Math.round((size * 1000) / ms);
}

export function classifyDownloadNetwork(props: {
  already_ready?: boolean;
  failed?: boolean;
  average_bps?: number | null;
  duration_ms?: number | null;
  bytes?: number | null;
}): UpdateNetworkClass {
  if (props.already_ready) return 'cache';
  if (props.failed) return 'failed';
  const averageBps = props.average_bps ?? computeAverageBps(props.bytes, props.duration_ms);
  if (averageBps == null) {
    const duration = nonNegInt(props.duration_ms);
    if (duration != null && duration >= SLOW_CHECK_MS) return 'slow';
    return 'unknown';
  }
  if (averageBps < SLOW_DOWNLOAD_BPS) return 'slow';
  if (averageBps >= FAST_DOWNLOAD_BPS) return 'fast';
  return 'ok';
}

export function classifyCheckNetwork(durationMs: number, status: UpdateCheckStatus): UpdateNetworkClass {
  if (status === 'failed') return 'failed';
  if (durationMs >= SLOW_CHECK_MS) return 'slow';
  if (durationMs <= 1_500) return 'fast';
  return 'ok';
}

function accessContextProps(extra?: UpdateTelemetryProps): UpdateTelemetryProps {
  return {
    feature: 'desktop_update',
    cdn_host: UPDATE_CDN_HOST,
    locale: clientLocale(),
    tz_offset_min: clientTimezoneOffsetMinutes(),
    ...extra,
  };
}

export function normalizeUpdateTelemetrySource(raw?: string | null): UpdateTelemetrySource {
  switch (raw) {
    case 'startup':
    case 'titlebar':
    case 'about':
    case 'menu':
    case 'modal':
      return raw;
    default:
      return 'unknown';
  }
}

export function classifyUpdateError(error: unknown): {
  error_code: string;
  failure_code: string | null;
} {
  const message = error instanceof Error ? error.message : typeof error === 'string' ? error : '';
  if (message.startsWith(AUTO_INSTALL_UNSUPPORTED_ERROR)) {
    const reason = message.slice(AUTO_INSTALL_UNSUPPORTED_ERROR.length + 1) || 'metadata_unavailable';
    return { error_code: 'install_blocked', failure_code: reason };
  }
  if (message.startsWith(INSTALL_NOT_ATTEMPTED_ERROR)) {
    return { error_code: 'package_not_ready', failure_code: null };
  }
  if (/network|fetch|timeout|timed out|dns|econn|enotfound|offline/i.test(message)) {
    return { error_code: 'network', failure_code: null };
  }
  if (/signature|pubkey|\.sig|verif/i.test(message)) {
    return { error_code: 'verify_failed', failure_code: null };
  }
  if (/404|not found|no update/i.test(message)) {
    return { error_code: 'not_found', failure_code: null };
  }
  return { error_code: 'unknown', failure_code: null };
}

function baseProps(extra?: UpdateTelemetryProps): UpdateTelemetryProps {
  return accessContextProps(extra);
}

export function trackUpdateCheckCompleted(props: {
  source: UpdateTelemetrySource;
  status: UpdateCheckStatus;
  duration_ms: number;
  from_version?: string | null;
  to_version?: string | null;
  error_code?: string | null;
}): FunnelEvent {
  const durationMs = Math.max(0, Math.round(props.duration_ms));
  return trackFunnelEvent(
    'update_check_completed',
    baseProps({
      source: props.source,
      status: props.status,
      duration_ms: durationMs,
      from_version: normalizeVersion(props.from_version),
      to_version: normalizeVersion(props.to_version),
      error_code: props.error_code ?? null,
      network_class: classifyCheckNetwork(durationMs, props.status),
    })
  );
}

export function trackUpdatePromptShown(props: {
  source: UpdateTelemetrySource;
  from_version?: string | null;
  to_version?: string | null;
}): FunnelEvent {
  return trackFunnelEvent(
    'update_prompt_shown',
    baseProps({
      source: props.source,
      from_version: normalizeVersion(props.from_version),
      to_version: normalizeVersion(props.to_version),
    })
  );
}

export function trackUpdateDownloadStarted(props: {
  source: UpdateTelemetrySource;
  from_version?: string | null;
  to_version?: string | null;
}): FunnelEvent {
  return trackFunnelEvent(
    'update_download_started',
    baseProps({
      source: props.source,
      from_version: normalizeVersion(props.from_version),
      to_version: normalizeVersion(props.to_version),
    })
  );
}

export function trackUpdateDownloadSucceeded(props: {
  source: UpdateTelemetrySource;
  duration_ms: number;
  from_version?: string | null;
  to_version?: string | null;
  bytes_total?: number | null;
  peak_bps?: number | null;
  already_ready?: boolean;
}): FunnelEvent {
  const durationMs = Math.max(0, Math.round(props.duration_ms));
  const bytesTotal = nonNegInt(props.bytes_total);
  const alreadyReady = props.already_ready ?? false;
  const averageBps = alreadyReady ? null : computeAverageBps(bytesTotal, durationMs);
  const peakBps = nonNegInt(props.peak_bps);
  return trackFunnelEvent(
    'update_download_succeeded',
    baseProps({
      source: props.source,
      duration_ms: durationMs,
      from_version: normalizeVersion(props.from_version),
      to_version: normalizeVersion(props.to_version),
      bytes_total: bytesTotal,
      bytes_transferred: bytesTotal,
      average_bps: averageBps,
      peak_bps: alreadyReady ? null : peakBps,
      already_ready: alreadyReady,
      network_class: classifyDownloadNetwork({
        already_ready: alreadyReady,
        average_bps: averageBps,
        duration_ms: durationMs,
        bytes: bytesTotal,
      }),
    })
  );
}

export function trackUpdateDownloadFailed(props: {
  source: UpdateTelemetrySource;
  duration_ms: number;
  from_version?: string | null;
  to_version?: string | null;
  bytes_total?: number | null;
  bytes_transferred?: number | null;
  peak_bps?: number | null;
  error: unknown;
}): FunnelEvent {
  const classified = classifyUpdateError(props.error);
  const durationMs = Math.max(0, Math.round(props.duration_ms));
  const bytesTransferred = nonNegInt(props.bytes_transferred);
  const averageBps = computeAverageBps(bytesTransferred, durationMs);
  return trackFunnelEvent(
    'update_download_failed',
    baseProps({
      source: props.source,
      duration_ms: durationMs,
      from_version: normalizeVersion(props.from_version),
      to_version: normalizeVersion(props.to_version),
      bytes_total: nonNegInt(props.bytes_total),
      bytes_transferred: bytesTransferred,
      average_bps: averageBps,
      peak_bps: nonNegInt(props.peak_bps),
      error_code: classified.error_code,
      failure_code: classified.failure_code,
      network_class: classifyDownloadNetwork({
        failed: true,
        average_bps: averageBps,
        duration_ms: durationMs,
        bytes: bytesTransferred,
      }),
    })
  );
}

export function trackUpdateInstallStarted(props: {
  source: UpdateTelemetrySource;
  from_version?: string | null;
  to_version?: string | null;
}): FunnelEvent {
  return trackFunnelEvent(
    'update_install_started',
    baseProps({
      source: props.source,
      from_version: normalizeVersion(props.from_version),
      to_version: normalizeVersion(props.to_version),
    })
  );
}

export function trackUpdateInstallFailed(props: {
  source: UpdateTelemetrySource;
  duration_ms: number;
  from_version?: string | null;
  to_version?: string | null;
  phase?: string | null;
  error: unknown;
}): FunnelEvent {
  const classified = classifyUpdateError(props.error);
  const name =
    classified.error_code === 'install_blocked' ? 'update_install_blocked' : 'update_install_failed';
  return trackFunnelEvent(
    name,
    baseProps({
      source: props.source,
      duration_ms: Math.max(0, Math.round(props.duration_ms)),
      from_version: normalizeVersion(props.from_version),
      to_version: normalizeVersion(props.to_version),
      phase: props.phase ?? null,
      error_code: classified.error_code,
      failure_code: classified.failure_code,
      failure_class: classified.error_code === 'install_blocked' ? 'preflight' : 'install',
    })
  );
}

export function markPendingUpdateApply(fromVersion: string, toVersion: string): void {
  const from = normalizeVersion(fromVersion);
  const to = normalizeVersion(toVersion);
  if (!from || !to || !canUseStorage()) return;
  const pending: PendingApply = {
    fromVersion: from,
    toVersion: to,
    markedAt: new Date().toISOString(),
  };
  try {
    window.localStorage.setItem(PENDING_APPLY_KEY, JSON.stringify(pending));
  } catch {
    // ignore
  }
}

export function clearPendingUpdateApply(): void {
  if (!canUseStorage()) return;
  try {
    window.localStorage.removeItem(PENDING_APPLY_KEY);
  } catch {
    // ignore
  }
}

function readPendingApply(): PendingApply | null {
  if (!canUseStorage()) return null;
  try {
    const raw = window.localStorage.getItem(PENDING_APPLY_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as PendingApply;
    if (
      !parsed ||
      typeof parsed.fromVersion !== 'string' ||
      typeof parsed.toVersion !== 'string' ||
      typeof parsed.markedAt !== 'string'
    ) {
      return null;
    }
    return parsed;
  } catch {
    return null;
  }
}

/**
 * After relaunch, confirm the running bundle matches the version we tried to
 * install. Emits once per pending marker (stable event id).
 */
export async function maybeTrackUpdateApplied(): Promise<FunnelEvent | null> {
  if (!isDesktopShell()) return null;
  const pending = readPendingApply();
  if (!pending) return null;

  let currentVersion = '';
  try {
    currentVersion = await tauriUpdateCurrentVersion();
  } catch {
    return null;
  }
  const current = normalizeVersion(currentVersion);
  if (!current) return null;

  if (current !== pending.toVersion) {
    clearPendingUpdateApply();
    trackFunnelEvent(
      'update_install_failed',
      baseProps({
        source: 'startup',
        from_version: pending.fromVersion,
        to_version: pending.toVersion,
        status: current,
        error_code: 'did_not_apply',
        failure_class: 'apply',
        cold_start: true,
      })
    );
    return null;
  }

  const eventId = `update:update_applied:${pending.fromVersion}->${pending.toVersion}`;
  const event = trackFunnelEventOnce(
    'update_applied',
    eventId,
    baseProps({
      source: 'startup',
      from_version: pending.fromVersion,
      to_version: pending.toVersion,
      cold_start: true,
    })
  );
  clearPendingUpdateApply();
  return event;
}

export function resetUpdateTelemetryForTests(): void {
  clearPendingUpdateApply();
}
