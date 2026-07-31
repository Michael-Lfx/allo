/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('./PresetCard.tsx', import.meta.url), 'utf8');

describe('PresetCard visual hierarchy', () => {
  test('uses a compact neutral management card surface', () => {
    expect(source.includes("'group relative flex flex-col rd-8px border border-solid p-14px cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgba(var(--primary-6),0.28)]'")).toBe(true);
    expect(source.includes('min-h-[214px]')).toBe(false);
    expect(source.includes("'border-[var(--color-border-2)] bg-[var(--color-bg-2)] hover:border-[var(--color-border-3)]")).toBe(true);
    expect(source.includes('hover:shadow')).toBe(false);
  });

  test('pins the text actions to the bottom and keeps their icon and label centered without a separator', () => {
    expect(source.includes('mt-auto pt-12px flex min-h-36px items-center justify-end gap-12px')).toBe(true);
    expect(source.includes('border-t border-solid')).toBe(false);
    expect(source.includes('inline-flex items-center gap-4px border-0 bg-transparent p-0 leading-none text-12px')).toBe(true);
  });

  test('lets short descriptions use their natural height while clamping overflow at two lines', () => {
    expect(source.includes('WebkitLineClamp: 2')).toBe(true);
    expect(source.includes('min-h-[36px]')).toBe(false);
  });
});
