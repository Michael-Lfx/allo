/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';

import { parseConversationId } from '@/common/types/ids';

import { isConversationOnScreen } from './conversationNotifyPolicy';

const CONVERSATION_ID = parseConversationId('0190f5fe-7c00-7a00-8000-000000000001');
const OTHER_CONVERSATION_ID = parseConversationId('0190f5fe-7c00-7a00-8000-000000000002');
const TERMINAL_ID = '0190f5fe-7c00-7a00-8000-000000000003';

const decide = (overrides: Partial<Parameters<typeof isConversationOnScreen>[0]> = {}) =>
  isConversationOnScreen({
    visible: true,
    pathname: `/conversation/${CONVERSATION_ID}`,
    conversationId: CONVERSATION_ID,
    ...overrides,
  });

describe('isConversationOnScreen', () => {
  test('is true only while that exact conversation is visible on screen', () => {
    expect(decide()).toBe(true);
    // Trailing slash comes from the same session route.
    expect(decide({ pathname: `/conversation/${CONVERSATION_ID}/` })).toBe(true);
  });

  test('is false for another conversation or a non-conversation route', () => {
    expect(decide({ pathname: `/conversation/${OTHER_CONVERSATION_ID}` })).toBe(false);
    expect(decide({ pathname: `/terminal/${TERMINAL_ID}` })).toBe(false);
    expect(decide({ pathname: '/settings' })).toBe(false);
    expect(decide({ pathname: '/conversation/not-an-id' })).toBe(false);
    expect(decide({ pathname: '' })).toBe(false);
  });

  test('is false while the page is hidden', () => {
    expect(decide({ visible: false })).toBe(false);
  });
});
