/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';

import type { IMessageTips, IMessageText } from '@/common/chat/chatLib';
import { parseConversationId, parseMessageId } from '@/common/types/ids';
import {
  hideInterruptedErrorTips,
  resolveTruncatedTurnRecovery,
  shouldOfferTruncatedContinuation,
} from './messageTruncatedContinuation';

const conversationId = parseConversationId('019b0000-0000-7000-8000-000000000921');
const userMessageId = parseMessageId('019b0000-0000-7000-8000-000000000922');
const errorMessageId = parseMessageId('019b0000-0000-7000-8000-000000000923');

const userMessage: IMessageText = {
  id: 'user',
  msg_id: userMessageId,
  conversation_id: conversationId,
  type: 'text',
  content: { content: '继续' },
  position: 'right',
  created_at: 100,
};

const errorMessage = (error?: IMessageTips['content']['error'], recovery?: IMessageTips['content']['recovery']): IMessageTips => ({
  id: 'error',
  msg_id: errorMessageId,
  conversation_id: conversationId,
  type: 'tips',
  content: {
    content: error?.message ?? 'failed',
    type: 'error',
    ...(error ? { error } : {}),
    ...(recovery ? { recovery } : {}),
  },
  position: 'center',
  created_at: 200,
});

describe('truncated-turn continuation recovery', () => {
  test('keeps a server-authored recovery payload', () => {
    const recovery = {
      kind: 'continue_truncated' as const,
      source_message_id: userMessageId,
      failure_code: 'user_llm_provider_network_error' as const,
    };
    expect(
      resolveTruncatedTurnRecovery(
        errorMessage(
          {
            message: 'Could not reach the model provider',
            code: 'USER_LLM_PROVIDER_NETWORK_ERROR',
            retryable: true,
          },
          recovery
        ),
        [userMessage]
      )
    ).toEqual(recovery);
  });

  test('synthesizes continue-from-progress for a timeout card without recovery', () => {
    expect(
      resolveTruncatedTurnRecovery(
        errorMessage({
          message: 'The model provider did not respond in time',
          code: 'USER_LLM_PROVIDER_TIMEOUT',
          retryable: true,
        }),
        [userMessage]
      )
    ).toEqual({
      kind: 'continue_truncated',
      source_message_id: userMessageId,
      failure_code: 'user_llm_provider_timeout',
    });
  });

  test('does not synthesize recovery for a non-resumable provider fault', () => {
    expect(
      resolveTruncatedTurnRecovery(
        errorMessage({
          message: 'auth failed',
          code: 'USER_LLM_PROVIDER_AUTH_FAILED',
          retryable: false,
        }),
        [userMessage]
      )
    ).toBeUndefined();
  });

  test('offers continue for a synthesized timeout even when retryable was omitted', () => {
    const message = errorMessage({
      message: 'The model provider did not respond in time',
      code: 'USER_LLM_PROVIDER_TIMEOUT',
    });
    const recovery = resolveTruncatedTurnRecovery(message, [userMessage]);
    expect(shouldOfferTruncatedContinuation(message, recovery, true, false)).toBe(true);
  });

  test('hides matching error tips after continue so the card leaves the transcript', () => {
    const timeout = errorMessage({
      message: 'timed out',
      code: 'USER_LLM_PROVIDER_TIMEOUT',
      retryable: true,
    });
    const hidden = hideInterruptedErrorTips([userMessage, timeout], userMessageId, userMessage.created_at);
    expect(hidden[1]?.hidden).toBe(true);
    expect(hidden[0]?.hidden).not.toBe(true);
  });
});
