/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(
  new URL('./LearningModelSelector.tsx', import.meta.url),
  'utf8',
);

describe('learning explicit model preference', () => {
  test('uses ChatModelPickerMenu and resolves fallback auto/cloud model when unset', () => {
    expect(source.includes('ChatModelPickerMenu')).toBe(true);
    expect(source.includes('buildChatModelPickerViewModel(groups)')).toBe(true);
    expect(source.includes('modelPicker.autoModels')).toBe(true);
    expect(source.includes('allChatModelOptions(modelPicker)')).toBe(true);
    expect(source.includes('findChatModelOption')).toBe(true);
  });

  test('does not contain broken __default__ menu item that resets to null', () => {
    expect(source.includes("key='__default__'")).toBe(false);
    expect(source.includes('onChange(null)')).toBe(false);
  });

  test('marks unavailable stored choice when missing from catalog', () => {
    expect(source.includes("t('learning.form.modelUnavailable')")).toBe(true);
    expect(source.includes("status={choiceUnavailable ? 'warning' : undefined}")).toBe(true);
    expect(source.includes("t('learning.form.modelUnavailableHint')")).toBe(true);
  });
});
