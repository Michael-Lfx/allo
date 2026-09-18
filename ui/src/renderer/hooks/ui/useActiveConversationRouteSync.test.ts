/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';

import { resolveActiveConversationId } from './useActiveConversationRouteSync';

const CONVERSATION_ID = '0190f5fe-7c00-7a00-8000-000000000001';
const TERMINAL_ID = '0190f5fe-7c00-7a00-8000-000000000002';

describe('resolveActiveConversationId', () => {
  test('resolves the conversation detail route, with or without a trailing slash', () => {
    expect(resolveActiveConversationId(`/conversation/${CONVERSATION_ID}`)).toBe(CONVERSATION_ID);
    expect(resolveActiveConversationId(`/conversation/${CONVERSATION_ID}/`)).toBe(CONVERSATION_ID);
  });

  test('returns null for terminal, other, and malformed routes', () => {
    for (const pathname of [
      `/terminal/${TERMINAL_ID}`,
      '/guid',
      '/settings',
      '/conversation/not-an-id',
      '',
    ]) {
      expect(resolveActiveConversationId(pathname)).toBeNull();
    }
  });
});
