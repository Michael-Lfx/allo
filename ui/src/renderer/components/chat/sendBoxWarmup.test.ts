/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';

import { shouldWarmComposerOnFocus } from './sendBoxWarmup';

describe('composer focus warmup (problem 2d: no gate contention from voice)', () => {
  test('voice conversations never warm the chat runtime on focus', () => {
    expect(
      shouldWarmComposerOnFocus({
        isVoiceConversation: true,
        conversationId: 'conv-voice',
        warmedConversationId: null,
      })
    ).toBe(false);
  });

  test('ordinary conversations warm once per conversation', () => {
    expect(
      shouldWarmComposerOnFocus({
        isVoiceConversation: false,
        conversationId: 'conv-chat',
        warmedConversationId: null,
      })
    ).toBe(true);
    expect(
      shouldWarmComposerOnFocus({
        isVoiceConversation: false,
        conversationId: 'conv-chat',
        warmedConversationId: 'conv-chat',
      })
    ).toBe(false);
  });

  test('missing conversation id is never warmed', () => {
    expect(
      shouldWarmComposerOnFocus({
        isVoiceConversation: false,
        conversationId: null,
        warmedConversationId: null,
      })
    ).toBe(false);
  });
});
