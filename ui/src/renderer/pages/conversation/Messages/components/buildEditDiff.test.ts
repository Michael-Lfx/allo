/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { buildEditDiffPreview } from './buildEditDiff';

describe('buildEditDiffPreview', () => {
  test('builds a hunk preview from exact-text Edit args', () => {
    const preview = buildEditDiffPreview({
      file_path: 'src/app.ts',
      old_string: 'const a = 1;',
      new_string: 'const a = 2;',
    });

    expect(preview?.displayName).toBe('app.ts');
    expect(preview?.insertions).toBeGreaterThan(0);
    expect(preview?.deletions).toBeGreaterThan(0);
    expect(preview?.hunks[0]?.lines.some((line) => line.kind === 'insert')).toBe(true);
  });

  test('builds stacked hunks from an edits array, including anchor new_text', () => {
    const preview = buildEditDiffPreview({
      file_path: 'src/catalog.ts',
      edits: [
        { old_string: 'alpha', new_string: 'ALPHA' },
        { anchor: '12:abcd', new_text: 'beta' },
      ],
    });

    expect(preview?.displayName).toBe('catalog.ts');
    expect(preview?.insertions).toBeGreaterThan(0);
    expect(preview?.hunks.length).toBeGreaterThan(0);
  });

  test('treats Write content as an all-insert preview', () => {
    const preview = buildEditDiffPreview({
      file_path: 'README.md',
      content: '# Hello\n',
    });

    expect(preview?.displayName).toBe('README.md');
    expect(preview?.deletions).toBe(0);
    expect(preview?.insertions).toBeGreaterThan(0);
  });

  test('parses JSON input strings used by process receipts', () => {
    const preview = buildEditDiffPreview(
      JSON.stringify({
        file_path: 'pkg/mod.rs',
        old_string: 'fn a() {}',
        new_string: 'fn b() {}',
      })
    );

    expect(preview?.displayName).toBe('mod.rs');
  });
});
