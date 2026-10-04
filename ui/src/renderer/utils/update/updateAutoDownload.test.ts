/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import * as bunTest from 'bun:test';

const { beforeEach, describe, expect, test } = bunTest;
const mock = (bunTest as unknown as { mock: { module: (specifier: string, factory: () => unknown) => void } }).mock;

type StatusListener = (evt: Record<string, unknown>) => void;

const listeners: StatusListener[] = [];
const events: Array<{ name: string; props: Record<string, unknown> }> = [];
const readyVersions: string[] = [];

let checkData: Record<string, unknown> = {};
let downloadImpl: () => Promise<{ success: boolean; msg?: string }> = async () => ({ success: true });
let checkCalls = 0;
let downloadCalls = 0;

const emit = (evt: Record<string, unknown>) => listeners.forEach((l) => l(evt));

mock.module('@/common', () => ({
  ipcBridge: {
    autoUpdate: {
      status: { on: (cb: StatusListener) => listeners.push(cb) },
      check: {
        invoke: async () => {
          checkCalls++;
          return { success: true, data: checkData };
        },
      },
      download: {
        invoke: () => {
          downloadCalls++;
          return downloadImpl();
        },
      },
    },
  },
}));

mock.module('@/common/adapter/tauriUpdater', () => ({
  tauriUpdateCurrentVersion: async () => '1.0.0',
}));

mock.module('@renderer/hooks/system/useUpdateAvailability', () => ({
  reportUpdateReady: (version?: string) => {
    if (version) readyVersions.push(version);
  },
}));

const record = (name: string) => (props: Record<string, unknown>) => {
  events.push({ name, props });
};

mock.module('@renderer/utils/analytics/updateTelemetry', () => ({
  trackUpdateDownloadStarted: record('started'),
  trackUpdateDownloadProgress: record('progress'),
  trackUpdateDownloadSucceeded: record('succeeded'),
  trackUpdateDownloadFailed: record('failed'),
}));

const {
  RECHECK_COOLDOWN_MS,
  ensureUpdateDownload,
  isBackgroundDownload,
  maybeAutoDownloadUpdate,
  resetUpdateAutoDownloadForTests,
} = await import('./updateAutoDownload');

const names = () => events.map((e) => e.name);

beforeEach(() => {
  resetUpdateAutoDownloadForTests();
  events.length = 0;
  readyVersions.length = 0;
  checkCalls = 0;
  downloadCalls = 0;
  checkData = { updateInfo: { version: '1.1.0' }, retainedVersion: null, packageState: 'idle' };
  downloadImpl = async () => ({ success: true });
  (globalThis as { localStorage?: unknown }).localStorage = { getItem: () => null };
});

describe('background update auto-download', () => {
  test('the first attempt is not blocked by the cooldown, even early in the process lifetime', async () => {
    await maybeAutoDownloadUpdate(5 * 60 * 1000);
    expect(downloadCalls).toBe(1);
  });

  test('a second attempt inside the cooldown does not re-check', async () => {
    await maybeAutoDownloadUpdate(1_000);
    await maybeAutoDownloadUpdate(1_000 + RECHECK_COOLDOWN_MS - 1);
    expect(checkCalls).toBe(1);
    await maybeAutoDownloadUpdate(1_000 + RECHECK_COOLDOWN_MS + 1);
    expect(checkCalls).toBe(2);
  });

  test('concurrent callers share one native download', async () => {
    let release!: () => void;
    downloadImpl = () =>
      new Promise((resolve) => {
        release = () => resolve({ success: true });
      });
    const a = ensureUpdateDownload();
    const b = ensureUpdateDownload();
    expect(a).toBe(b);
    await new Promise((r) => setTimeout(r, 0));
    release();
    expect(await a).toBe(true);
    expect(downloadCalls).toBe(1);
    expect(checkCalls).toBe(1);
  });

  test('no download when nothing is available, retained, or already downloading', async () => {
    checkData = {};
    expect(await ensureUpdateDownload()).toBe(false);

    checkData = { updateInfo: { version: '1.1.0' }, packageState: 'downloading' };
    expect(await ensureUpdateDownload()).toBe(false);

    expect(downloadCalls).toBe(0);
    expect(events).toHaveLength(0);
  });

  test('an already retained package lights the ready badge without downloading', async () => {
    checkData = { updateInfo: { version: '1.1.0' }, retainedVersion: '1.1.0', packageState: 'ready' };
    expect(await ensureUpdateDownload()).toBe(false);
    expect(downloadCalls).toBe(0);
    expect(readyVersions).toEqual(['1.1.0']);
  });

  test('reports progress checkpoints once each and a single success, then marks ready', async () => {
    downloadImpl = async () => {
      for (const percent of [10, 30, 30, 60, 100]) {
        emit({
          status: 'downloading',
          version: '1.1.0',
          progress: { percent, transferred: percent, total: 100, bytesPerSecond: 5 },
        });
      }
      emit({ status: 'downloaded', version: '1.1.0' });
      return { success: true };
    };
    await ensureUpdateDownload();

    const progress = events.filter((e) => e.name === 'progress').map((e) => e.props.percent);
    expect(progress).toEqual([25, 50, 75, 100]);
    expect(names().filter((n) => n === 'started')).toHaveLength(1);
    expect(names().filter((n) => n === 'succeeded')).toHaveLength(1);
    expect(names()).not.toContain('failed');
    expect(readyVersions).toEqual(['1.1.0']);
  });

  test('a failed invoke is reported exactly once', async () => {
    downloadImpl = async () => ({ success: false, msg: 'boom' });
    expect(await ensureUpdateDownload()).toBe(false);
    expect(names().filter((n) => n === 'failed')).toHaveLength(1);
  });

  test('a terminal error frame followed by a failed invoke is not double counted', async () => {
    downloadImpl = async () => {
      emit({ status: 'error', version: '1.1.0', error: 'net' });
      return { success: false, msg: 'net' };
    };
    await ensureUpdateDownload();
    expect(names().filter((n) => n === 'failed')).toHaveLength(1);
  });

  test('frames from a transfer this module does not own produce no telemetry but still mark ready', () => {
    emit({
      status: 'downloading',
      version: '2.0.0',
      progress: { percent: 100, transferred: 1, total: 1, bytesPerSecond: 1 },
    });
    emit({ status: 'downloaded', version: '2.0.0' });
    expect(events).toHaveLength(0);
    expect(readyVersions).toEqual(['2.0.0']);
  });

  test('other listeners can tell a background transfer apart during terminal dispatch only', async () => {
    const seen: boolean[] = [];
    listeners.push((evt) => {
      if (evt.status === 'downloaded') seen.push(isBackgroundDownload('1.1.0'));
    });
    downloadImpl = async () => {
      emit({ status: 'downloaded', version: '1.1.0' });
      return { success: true };
    };
    await ensureUpdateDownload();
    expect(seen).toEqual([true]);
    expect(isBackgroundDownload('1.1.0')).toBe(false);
  });
});
