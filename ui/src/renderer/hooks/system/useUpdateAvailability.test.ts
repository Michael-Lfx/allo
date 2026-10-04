/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import {
  getUpdateAvailabilitySnapshot,
  reportNoUpdateAvailable,
  reportUpdateAvailable,
  reportUpdateReady,
} from './useUpdateAvailability';

describe('shared update availability', () => {
  test('publishes available versions and clears them after a no-update result', () => {
    reportNoUpdateAvailable();
    expect(getUpdateAvailabilitySnapshot()).toEqual({ available: false, readyToInstall: false });

    reportUpdateAvailable('0.2.22');
    expect(getUpdateAvailabilitySnapshot()).toEqual({ available: true, readyToInstall: false, version: '0.2.22' });

    reportNoUpdateAvailable();
    expect(getUpdateAvailabilitySnapshot()).toEqual({ available: false, readyToInstall: false });
  });

  test('supports update events that do not include a version', () => {
    reportUpdateAvailable();
    expect(getUpdateAvailabilitySnapshot()).toEqual({ available: true, readyToInstall: false });

    reportNoUpdateAvailable();
  });

  test('a ready package survives a re-check of the same version but not a newer one', () => {
    reportUpdateReady('0.2.22');
    reportUpdateAvailable('0.2.22');
    expect(getUpdateAvailabilitySnapshot().readyToInstall).toBe(true);

    reportUpdateAvailable('0.2.23');
    expect(getUpdateAvailabilitySnapshot().readyToInstall).toBe(false);

    reportNoUpdateAvailable();
  });
});
