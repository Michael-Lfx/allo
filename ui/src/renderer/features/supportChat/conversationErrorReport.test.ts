/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import {
  buildConversationErrorReportMetadata,
  getConversationErrorReportContextKey,
  type ConversationErrorReportContext,
} from './conversationErrorReport';

const context: ConversationErrorReportContext = {
  error: {
    message: 'The model provider is temporarily unavailable',
    incident_id: '019c0000-0000-7000-8000-000000000001',
    code: 'USER_LLM_PROVIDER_GATEWAY_ERROR',
    ownership: 'user_llm_provider',
    detail: 'Authorization: Bearer secret\nProvider response was rejected',
    workspacePath: 'C:\\private\\workspace',
    retryable: true,
    feedback_recommended: false,
    resolution: { kind: 'retry' },
  },
  conversationId: '019c0000-0000-7000-8000-000000000002',
  messageId: '019c0000-0000-7000-8000-000000000003',
  turnId: '019c0000-0000-7000-8000-000000000004',
  occurredAt: '2026-07-30T03:21:00.000Z',
};

describe('conversation error support report', () => {
  test('builds stable correlation metadata with safe detail and without workspace paths', () => {
    const metadata = buildConversationErrorReportMetadata(context);

    expect(metadata).toEqual({
      schemaVersion: 1,
      reportType: 'conversation_error',
      source: 'error_card_feedback',
      incidentId: context.error.incident_id,
      error: {
        message: context.error.message,
        code: context.error.code,
        ownership: context.error.ownership,
        retryable: true,
        feedbackRecommended: false,
        detail: 'Authorization: Bearer [REDACTED]\nProvider response was rejected',
        resolution: { kind: 'retry' },
      },
      correlation: {
        conversationId: context.conversationId,
        messageId: context.messageId,
        turnId: context.turnId,
        occurredAt: context.occurredAt,
      },
    });
    expect(JSON.stringify(metadata)).not.toContain('Bearer secret');
    expect(JSON.stringify(metadata)).not.toContain('private');
  });

  test('keeps the same context stable while changing report contexts', () => {
    expect(getConversationErrorReportContextKey(context)).toBe(
      `${context.conversationId}:${context.messageId}:${context.turnId}:${context.occurredAt}`
    );
    expect(
      getConversationErrorReportContextKey({ ...context, messageId: undefined })
    ).not.toBe(getConversationErrorReportContextKey(context));
  });
});
