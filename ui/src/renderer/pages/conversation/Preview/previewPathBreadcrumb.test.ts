/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { previewPathSegments } from './previewPathBreadcrumb';

describe('previewPathSegments', () => {
  test('returns workspace-relative folder hierarchy, not only the leaf name', () => {
    expect(previewPathSegments('C:/repo/.config/nextest.toml', 'C:/repo')).toEqual(['.config', 'nextest.toml']);
    expect(previewPathSegments('C:\\repo\\skills\\cron\\SKILL.md', 'C:\\repo')).toEqual([
      'skills',
      'cron',
      'SKILL.md',
    ]);
  });

  test('keeps already-relative paths intact', () => {
    expect(previewPathSegments('skills/cron/SKILL.md')).toEqual(['skills', 'cron', 'SKILL.md']);
    expect(previewPathSegments('SKILL.md')).toEqual(['SKILL.md']);
  });

  test('falls back to parent > file for absolute paths outside the workspace', () => {
    expect(previewPathSegments('D:/other/.config/nextest.toml', 'C:/repo')).toEqual(['.config', 'nextest.toml']);
  });
});
