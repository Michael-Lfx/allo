/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { formatCode } from './markdownUtils';

describe('formatCode', () => {
  test('keeps the same string when there is no trailing newline and it is not JSON', () => {
    const input = 'fn main() {}';
    expect(formatCode(input)).toBe(input);
  });

  test('strips a single trailing newline without JSON-pretty-printing rust', () => {
    expect(formatCode('fn main() {}\n')).toBe('fn main() {}');
  });

  test('pretty-prints JSON fences', () => {
    expect(formatCode('{"a":1}')).toBe('{\n  "a": 1\n}');
  });
});
