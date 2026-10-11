/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { LocalModelRunStatus } from '@common/types/localModel';

/** Which primary action button the card footer shows for a given state. */
export interface ModelStoreCardActions {
  /** Show the download (install) button. */
  download: boolean;
  /** Show the download progress bar. */
  progress: boolean;
  /** Show the cancel-download button. */
  cancel: boolean;
  /** Show the green play button. */
  play: boolean;
  /** Show the stop button. */
  stop: boolean;
  /** Menu offers "launch config". */
  launchConfig: boolean;
  /** Menu offers "uninstall". */
  uninstall: boolean;
}

/**
 * Resolve the card action visibility, mirroring the herdsman models hub:
 * uninstalled → download; downloading → progress + cancel; installed → play;
 * starting/running → stop. The three-dot menu always offers details.
 */
export function resolveCardActions(
  status: LocalModelRunStatus,
  launchable = true
): ModelStoreCardActions {
  switch (status) {
    case 'uninstalled':
      return {
        download: true,
        progress: false,
        cancel: false,
        play: false,
        stop: false,
        launchConfig: false,
        uninstall: false,
      };
    case 'downloading':
      return {
        download: false,
        progress: true,
        cancel: true,
        play: false,
        stop: false,
        launchConfig: false,
        uninstall: false,
      };
    case 'installed':
      return {
        download: false,
        progress: false,
        cancel: false,
        play: launchable,
        stop: false,
        launchConfig: launchable,
        uninstall: true,
      };
    case 'starting':
    case 'running':
      return {
        download: false,
        progress: false,
        cancel: false,
        play: false,
        stop: true,
        launchConfig: true,
        uninstall: true,
      };
  }
}

/** Whether the modal flow should offer to stop (used by the launch dialog). */
export function isModelRunning(status: LocalModelRunStatus): boolean {
  return status === 'running' || status === 'starting';
}
