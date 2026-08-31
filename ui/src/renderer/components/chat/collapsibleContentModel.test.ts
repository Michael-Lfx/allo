/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { shouldCollapseContent } from './collapsibleContentModel';

describe('shouldCollapseContent', () => {
  test('ignores content at or within the one pixel tolerance', () => {
    expect(shouldCollapseContent(200, 200)).toBe(false);
    expect(shouldCollapseContent(201, 200)).toBe(false);
  });

  test('collapses content that exceeds the tolerance', () => {
    expect(shouldCollapseContent(202, 200)).toBe(true);
  });
});
