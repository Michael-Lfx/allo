/**
 * One-shot restart handshake used by the development supervisor.
 *
 * Tauri's Windows event loop does not preserve the application exit code all
 * the way to the process spawned by the Tauri CLI. The marker is therefore
 * the authoritative signal for a development restart; the exit code is only
 * useful for reporting a normal child failure.
 */

import { randomBytes } from 'node:crypto';
import {
  lstatSync,
  mkdtempSync,
  readFileSync,
  rmSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

export const RESTART_MARKER_VERSION = 1;
export const RESTART_MARKER_MAX_BYTES = 1024;

function removeMarker(markerPath) {
  try {
    // rmSync never follows a symlink at the marker path. Do not use
    // recursive removal here: a malicious or corrupted marker must never
    // turn into a request to remove another directory tree.
    rmSync(markerPath, { force: true, recursive: false });
  } catch {
    // A missing or already-removed marker is equivalent to a consumed marker.
  }
}

export function createRestartControlDirectory() {
  const directory = mkdtempSync(join(tmpdir(), 'nomifun-dev-restart-'));
  return {
    directory,
    markerPath: join(directory, 'restart.json'),
  };
}

export function createRestartSignal(controlDirectory) {
  const token = randomBytes(32).toString('hex');
  const markerPath = controlDirectory.markerPath;

  // A previous Tauri child must not be able to make the next child restart.
  removeMarker(markerPath);

  return {
    markerPath,
    token,
    env: {
      NOMI_DEV_RESTART_MARKER: markerPath,
      NOMI_DEV_RESTART_TOKEN: token,
    },
    consume() {
      return consumeRestartMarker({ markerPath, token });
    },
    cleanup() {
      removeMarker(markerPath);
    },
  };
}

export function consumeRestartMarker({ markerPath, token }) {
  let valid = false;
  try {
    const metadata = lstatSync(markerPath);
    if (metadata.isSymbolicLink() || !metadata.isFile()) return false;
    if (metadata.size > RESTART_MARKER_MAX_BYTES) return false;

    const contents = readFileSync(markerPath);
    if (contents.byteLength > RESTART_MARKER_MAX_BYTES) return false;

    const marker = JSON.parse(contents.toString('utf8'));
    valid =
      marker?.version === RESTART_MARKER_VERSION &&
      marker?.token === token &&
      Number.isSafeInteger(marker?.app_pid) &&
      marker.app_pid > 0;
  } catch {
    valid = false;
  } finally {
    // Invalid markers are consumed too. This prevents a stale/corrupt file
    // from being interpreted by a later Tauri launch.
    removeMarker(markerPath);
  }
  return valid;
}

export function removeRestartControlDirectory(controlDirectory) {
  try {
    const metadata = lstatSync(controlDirectory.directory);
    if (metadata.isSymbolicLink() || !metadata.isDirectory()) return;
    rmSync(controlDirectory.directory, { force: true, recursive: true });
  } catch {
    // Best-effort cleanup of a directory created by this supervisor only.
  }
}
