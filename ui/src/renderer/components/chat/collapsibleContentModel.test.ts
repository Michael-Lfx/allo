/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { getCollapsibleContentLayout, shouldCollapseContent } from './collapsibleContentModel';

describe('shouldCollapseContent', () => {
  test('ignores content at or within the one pixel tolerance', () => {
    expect(shouldCollapseContent(200, 200)).toBe(false);
    expect(shouldCollapseContent(201, 200)).toBe(false);
  });

  test('collapses content that exceeds the tolerance', () => {
    expect(shouldCollapseContent(202, 200)).toBe(true);
  });

  test('does not clip or mask content after measurement says collapse is unnecessary', () => {
    expect(getCollapsibleContentLayout(true, false, true)).toEqual({
      shouldClip: false,
      shouldMask: false,
    });
  });

  test('keeps long content clipped while measurement is pending, without showing a mask early', () => {
    expect(getCollapsibleContentLayout(true, null, true)).toEqual({
      shouldClip: true,
      shouldMask: false,
    });
    expect(getCollapsibleContentLayout(true, true, true)).toEqual({
      shouldClip: true,
      shouldMask: true,
    });
    expect(getCollapsibleContentLayout(false, true, true)).toEqual({
      shouldClip: false,
      shouldMask: false,
    });
  });
});
