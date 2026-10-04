/**
 * Background auto-download coordinator for the in-app updater.
 *
 * Single owner of "an update package is fetched without the user asking".
 * - Concurrent callers share ONE native download (`inFlight`).
 * - Download telemetry is reported only while this module owns the transfer;
 *   `isBackgroundDownload` lets the modal skip frames of that same transfer.
 */
import { ipcBridge } from '@/common';
import { tauriUpdateCurrentVersion } from '@/common/adapter/tauriUpdater';
import { reportUpdateReady } from '@renderer/hooks/system/useUpdateAvailability';
import {
  trackUpdateDownloadFailed,
  trackUpdateDownloadProgress,
  trackUpdateDownloadStarted,
  trackUpdateDownloadSucceeded,
} from '@renderer/utils/analytics/updateTelemetry';

const PROGRESS_STEPS = [25, 50, 75, 100] as const;
export const RECHECK_COOLDOWN_MS = 10 * 60 * 1000;

let inFlight: Promise<boolean> | null = null;
let lastAttemptAt: number | null = null;

let ownedFrom = '';
let ownedTo: string | null = null;
let ownedStartedAt: number | null = null;
let peakBps = 0;
let transferred = 0;
let total = 0;
let stepIndex = 0;

const backgroundVersions = new Set<string>();

function ownsTransfer(): boolean {
  return ownedStartedAt != null;
}

function resetCounters(from: string, to: string): void {
  ownedFrom = from;
  ownedTo = to;
  ownedStartedAt = performance.now();
  peakBps = 0;
  transferred = 0;
  total = 0;
  stepIndex = 0;
}

function elapsedMs(): number {
  return ownedStartedAt != null ? performance.now() - ownedStartedAt : 0;
}

function releaseOwnership(): void {
  const version = ownedTo;
  ownedStartedAt = null;
  if (version) queueMicrotask(() => backgroundVersions.delete(version));
}

function reportFailure(error: unknown): void {
  trackUpdateDownloadFailed({
    source: 'background',
    duration_ms: elapsedMs(),
    from_version: ownedFrom,
    to_version: ownedTo,
    bytes_total: total || null,
    bytes_transferred: transferred || null,
    peak_bps: peakBps || null,
    error,
  });
  releaseOwnership();
}

/**
 * True while a status frame for `version` belongs to a background transfer.
 * Stays true for the whole synchronous dispatch of the terminal frame, so other
 * listeners of the same emitter can skip telemetry this module already sent.
 */
export function isBackgroundDownload(version?: string | null): boolean {
  return !!version && backgroundVersions.has(version);
}

ipcBridge.autoUpdate.status.on((evt) => {
  if (!evt) return;
  if (evt.status === 'downloaded' && evt.version) reportUpdateReady(evt.version);
  if (!ownsTransfer()) return;
  const toVersion = evt.version ?? ownedTo;

  if (evt.status === 'downloading' && evt.progress) {
    const { bytesPerSecond, transferred: received, total: contentLength, percent } = evt.progress;
    if (Number.isFinite(bytesPerSecond) && bytesPerSecond > peakBps) peakBps = bytesPerSecond;
    transferred = received;
    if (contentLength > 0) total = contentLength;
    while (stepIndex < PROGRESS_STEPS.length && percent >= PROGRESS_STEPS[stepIndex]) {
      const step = PROGRESS_STEPS[stepIndex++];
      trackUpdateDownloadProgress({
        source: 'background',
        from_version: ownedFrom,
        to_version: toVersion,
        percent: step,
        elapsed_ms: elapsedMs(),
        bytes_total: total || null,
      });
    }
    return;
  }

  if (evt.status === 'downloaded') {
    trackUpdateDownloadSucceeded({
      source: 'background',
      duration_ms: elapsedMs(),
      from_version: ownedFrom,
      to_version: toVersion,
      bytes_total: evt.progress?.total ?? (total || null),
      peak_bps: peakBps || null,
      already_ready: false,
    });
    releaseOwnership();
    return;
  }

  if (evt.status === 'error') reportFailure(evt.error || 'download_failed');
});

/**
 * Fetch the available update unless the native slot already holds it or is
 * already downloading it. Resolves `true` when a download ran to completion.
 * Never throws.
 */
export function ensureUpdateDownload(): Promise<boolean> {
  if (inFlight) return inFlight;

  const run = (async (): Promise<boolean> => {
    const fromVersion = await tauriUpdateCurrentVersion().catch(() => '');
    const includePrerelease = localStorage.getItem('update.includePrerelease') === 'true';
    const check = await ipcBridge.autoUpdate.check.invoke({ includePrerelease });
    const slot = check?.data;
    const info = slot?.updateInfo;
    if (!info) return false;
    if (slot?.retainedVersion) {
      reportUpdateReady(slot.retainedVersion);
      return false;
    }
    if (slot?.packageState === 'downloading') return false;

    resetCounters(fromVersion, info.version);
    backgroundVersions.add(info.version);
    trackUpdateDownloadStarted({
      source: 'background',
      from_version: fromVersion,
      to_version: info.version,
    });

    const result = await ipcBridge.autoUpdate.download.invoke();
    if (!result?.success) {
      if (ownsTransfer()) reportFailure(result?.msg || 'download_failed');
      return false;
    }
    return true;
  })();

  const tracked = run
    .catch((error) => {
      if (ownsTransfer()) reportFailure(error);
      return false;
    })
    .finally(() => {
      inFlight = null;
    });

  inFlight = tracked;
  return tracked;
}

/** Cooldown-gated entry for the launch timer and focus/online retries. */
export function maybeAutoDownloadUpdate(now: number = performance.now()): Promise<boolean> {
  if (inFlight) return inFlight;
  if (lastAttemptAt != null && now - lastAttemptAt < RECHECK_COOLDOWN_MS) return Promise.resolve(false);
  lastAttemptAt = now;
  return ensureUpdateDownload();
}

export function resetUpdateAutoDownloadForTests(): void {
  inFlight = null;
  lastAttemptAt = null;
  ownedFrom = '';
  ownedTo = null;
  ownedStartedAt = null;
  peakBps = 0;
  transferred = 0;
  total = 0;
  stepIndex = 0;
  backgroundVersions.clear();
}
