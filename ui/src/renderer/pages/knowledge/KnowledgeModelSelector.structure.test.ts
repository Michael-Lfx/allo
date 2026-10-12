/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(
  new URL('./KnowledgeModelSelector.tsx', import.meta.url),
  'utf8',
);

describe('knowledge explicit model preference', () => {
  test('uses ChatModelPickerMenu and resolves through the persisted-choice resolver', () => {
    expect(source.includes('ChatModelPickerMenu')).toBe(true);
    expect(source.includes('buildChatModelPickerViewModel(groups)')).toBe(true);
    expect(source.includes('resolvePersistedModelChoice(modelPicker, stored, catalogResolved)')).toBe(true);
    expect(source.includes('findChatModelOption')).toBe(true);
  });

  test('does not contain broken __default__ menu item that resets to null', () => {
    expect(source.includes("key='__default__'")).toBe(false);
    expect(source.includes('onChange(null)')).toBe(false);
  });

  test('marks unavailable stored choice when missing from catalog', () => {
    expect(source.includes("t('knowledge.form.modelUnavailable')")).toBe(true);
    expect(source.includes("status={choiceUnavailable ? 'warning' : undefined}")).toBe(true);
    expect(source.includes("t('knowledge.form.modelUnavailableHint')")).toBe(true);
  });
});
