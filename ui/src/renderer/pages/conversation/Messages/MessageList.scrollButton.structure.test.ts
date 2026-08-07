/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const messageListSource = readFileSync(new URL('./MessageList.tsx', import.meta.url), 'utf8');
const messageStyles = readFileSync(new URL('./messages.css', import.meta.url), 'utf8');

describe('message list scroll button', () => {
  test('declares its circular shape so the global button contract cannot square it', () => {
    expect(messageListSource.includes("className='message-list-scroll-button'")).toBe(true);
    expect(messageListSource.includes("data-button-shape='circle'")).toBe(true);
    expect(messageStyles.includes('.message-list-scroll-button {')).toBe(true);
    expect(messageStyles.includes('border-radius: 999px;')).toBe(true);
  });
});
