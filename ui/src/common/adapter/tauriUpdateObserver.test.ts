/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import type { AutoUpdateStatus } from '../update/updateTypes';
import type { TauriUpdatePackageStatus } from './tauriShell';
import { observeNativeDownload } from './tauriUpdateObserver';

const status = (
  state: TauriUpdatePackageStatus['state'],
  version: string | null,
  transferred = 0,
  total = 0
): TauriUpdatePackageStatus => ({ state, version, transferred, total });

const run = async (sequence: Array<TauriUpdatePackageStatus | null>, localActive: () => boolean = () => false) => {
  const frames: AutoUpdateStatus[] = [];
  let clock = 0;
  let index = 0;
  await observeNativeDownload({
    readStatus: async () => sequence[Math.min(index++, sequence.length - 1)],
    isLocalDownloadActive: localActive,
    emit: (frame) => frames.push(frame),
    sleep: async () => {
      clock += 1000;
    },
    now: () => clock,
  });
  return frames;
};

describe('native download observer', () => {
  test('replays progress then the downloaded frame when an orphaned download completes', async () => {
    const frames = await run([
      status('downloading', '1.1.0', 250, 1000),
      status('downloading', '1.1.0', 750, 1000),
      status('ready', '1.1.0'),
    ]);

    expect(frames.map((f) => f.status)).toEqual(['downloading', 'downloading', 'downloaded']);
    expect(frames[0].progress?.percent).toBe(25);
    expect(frames[1].progress?.percent).toBe(75);
    expect(frames[1].progress?.bytesPerSecond).toBe(500);
    expect(frames[2].version).toBe('1.1.0');
  });

  test('reports an error when the native download disappears without a ready package', async () => {
    const frames = await run([status('downloading', '1.1.0', 100, 1000), status('empty', null)]);
    expect(frames.map((f) => f.status)).toEqual(['downloading', 'error']);
    expect(frames[1].version).toBe('1.1.0');
  });

  test('reports an error when a different version becomes ready', async () => {
    const frames = await run([status('downloading', '1.1.0', 100, 1000), status('ready', '1.2.0')]);
    expect(frames.map((f) => f.status)).toEqual(['downloading', 'error']);
  });

  test('emits nothing when the slot is not downloading at the first look', async () => {
    expect(await run([status('ready', '1.1.0')])).toEqual([]);
    expect(await run([status('empty', null)])).toEqual([]);
    expect(await run([null])).toEqual([]);
  });

  test('stands down while this renderer owns the download', async () => {
    let calls = 0;
    const frames = await run([status('downloading', '1.1.0', 1, 10)], () => calls++ > 0);
    expect(frames).toEqual([]);
  });

  test('reports zero percent while the total is still unknown', async () => {
    const frames = await run([status('downloading', '1.1.0', 500, 0), status('ready', '1.1.0')]);
    expect(frames[0].progress?.percent).toBe(0);
  });
});
