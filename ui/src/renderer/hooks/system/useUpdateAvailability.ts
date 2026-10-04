/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useSyncExternalStore } from 'react';

export interface UpdateAvailabilitySnapshot {
  available: boolean;
  /** The native slot already holds this version's verified bytes. */
  readyToInstall: boolean;
  version?: string;
}

const NO_UPDATE: UpdateAvailabilitySnapshot = { available: false, readyToInstall: false };

let snapshot: UpdateAvailabilitySnapshot = NO_UPDATE;
const listeners = new Set<() => void>();

const emit = () => listeners.forEach((listener) => listener());

const setSnapshot = (next: UpdateAvailabilitySnapshot) => {
  if (
    snapshot.available === next.available &&
    snapshot.version === next.version &&
    snapshot.readyToInstall === next.readyToInstall
  )
    return;
  snapshot = next;
  emit();
};

/** Publish a successful update check so every app-level entry point stays in sync. */
export const reportUpdateAvailable = (version?: string) => {
  // Preserve a ready package for the SAME version: a re-check must not clear the
  // install affordance a background download just earned.
  const readyToInstall = snapshot.readyToInstall && snapshot.version === version;
  setSnapshot({ available: true, ...(version ? { version } : {}), readyToInstall });
};

/** Hide the app-level update entry after an authoritative check finds no update. */
export const reportNoUpdateAvailable = () => {
  setSnapshot(NO_UPDATE);
};

/**
 * The background download finished, so the update is installable without a
 * further user click. Surfaced on the titlebar badge so the ready state is
 * visible even if the user never opened the modal.
 */
export const reportUpdateReady = (version?: string) => {
  setSnapshot({ available: true, readyToInstall: true, ...(version ? { version } : {}) });
};

const subscribe = (listener: () => void): (() => void) => {
  listeners.add(listener);
  return () => listeners.delete(listener);
};

/** Non-React snapshot getter, also useful for focused store tests. */
export const getUpdateAvailabilitySnapshot = (): UpdateAvailabilitySnapshot => snapshot;

/** Shared, renderer-local update availability for persistent global UI. */
export const useUpdateAvailability = (): UpdateAvailabilitySnapshot =>
  useSyncExternalStore(subscribe, getUpdateAvailabilitySnapshot, getUpdateAvailabilitySnapshot);
