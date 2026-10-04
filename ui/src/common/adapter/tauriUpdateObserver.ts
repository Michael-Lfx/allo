/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { AutoUpdateStatus } from '../update/updateTypes';
import type { TauriUpdatePackageStatus } from './tauriShell';

export const OBSERVE_INTERVAL_MS = 1000;

export interface NativeDownloadObserverDeps {
  readStatus: () => Promise<TauriUpdatePackageStatus | null>;
  /** True while this renderer's own `download_update` invoke owns the transfer. */
  isLocalDownloadActive: () => boolean;
  emit: (status: AutoUpdateStatus) => void;
  sleep?: (ms: number) => Promise<void>;
  now?: () => number;
  intervalMs?: number;
}

const defaultSleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/**
 * Follow a native download this renderer does not own. The Rust download task
 * outlives a webview reload but its progress channel does not, so the reloaded
 * renderer would otherwise show "downloading" forever. This polls the
 * authoritative native slot and replays progress and the terminal outcome as the
 * same status frames an owned download emits.
 */
export async function observeNativeDownload(deps: NativeDownloadObserverDeps): Promise<void> {
  const { readStatus, isLocalDownloadActive, emit } = deps;
  const sleep = deps.sleep ?? defaultSleep;
  const now = deps.now ?? (() => performance.now());
  const intervalMs = deps.intervalMs ?? OBSERVE_INTERVAL_MS;

  let version: string | null = null;
  let lastBytes = 0;
  let lastTs = now();

  for (;;) {
    if (isLocalDownloadActive()) return;
    const status = await readStatus();
    if (!status || isLocalDownloadActive()) return;

    if (status.state === 'downloading' && status.version) {
      if (version !== status.version) {
        version = status.version;
        lastBytes = status.transferred;
        lastTs = now();
      }
      const ts = now();
      const dt = ts - lastTs;
      const bytesPerSecond = dt > 0 ? (Math.max(0, status.transferred - lastBytes) / dt) * 1000 : 0;
      lastBytes = status.transferred;
      lastTs = ts;
      emit({
        status: 'downloading',
        version,
        progress: {
          percent: status.total > 0 ? Math.min(100, (status.transferred / status.total) * 100) : 0,
          transferred: status.transferred,
          total: status.total,
          bytesPerSecond,
        },
      });
      await sleep(intervalMs);
      continue;
    }

    if (!version) return;
    if (status.state === 'ready' && status.version === version) {
      emit({ status: 'downloaded', version });
    } else if (status.state === 'empty' || status.state === 'ready') {
      emit({ status: 'error', version });
    }
    return;
  }
}
