/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { isObservationRetentionError } from './useAgentTraces';

describe('isObservationRetentionError', () => {
  test('requires observation_retention reason or code, not a bare 410', () => {
    expect(isObservationRetentionError({ status: 410 })).toBe(false);
    expect(
      isObservationRetentionError({
        status: 410,
        body: { error: 'gone' },
      })
    ).toBe(false);
    expect(
      isObservationRetentionError({
        status: 410,
        body: { code: 'OBSERVATION_RETENTION' },
      })
    ).toBe(true);
    expect(
      isObservationRetentionError({
        status: 500,
        body: { reason: 'observation_retention' },
      })
    ).toBe(true);
    expect(
      isObservationRetentionError({
        status: 410,
        body: { details: { reason: 'observation_retention' } },
      })
    ).toBe(true);
  });
});
