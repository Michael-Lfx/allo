/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { COMMERCIAL_PATH_FRAMES } from './commercialPathModel';

describe('commercial path prototype', () => {
  test('covers the six required conversion states', () => {
    expect(COMMERCIAL_PATH_FRAMES.map((frame) => frame.state).sort()).toEqual(
      [
        'first_user',
        'missing_model',
        'model_failure',
        'network_failure',
        'returning_user',
        'task_success',
      ].sort()
    );
    expect(new Set(COMMERCIAL_PATH_FRAMES.map((frame) => frame.scene)).size).toBeGreaterThanOrEqual(3);
  });
});
