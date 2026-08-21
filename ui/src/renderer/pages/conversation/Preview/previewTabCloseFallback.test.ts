/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { resolveActiveTabAfterClose } from './previewTabCloseFallback';

describe('resolveActiveTabAfterClose', () => {
  test('restores the previous active tab when it still exists', () => {
    const remaining = [{ id: 'files' }, { id: 'shell' }];
    expect(resolveActiveTabAfterClose(remaining, 'files')).toBe('files');
  });

  test('ignores previous id when that tab was also removed', () => {
    const remaining = [{ id: 'shell' }];
    expect(resolveActiveTabAfterClose(remaining, 'files')).toBe('shell');
  });

  test('falls back to the last remaining tab when there is no previous', () => {
    const remaining = [{ id: 'a' }, { id: 'c' }];
    expect(resolveActiveTabAfterClose(remaining, null)).toBe('c');
  });

  test('returns null when no tabs remain', () => {
    expect(resolveActiveTabAfterClose([], 'files')).toBe(null);
  });
});
