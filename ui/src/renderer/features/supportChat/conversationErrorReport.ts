/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { AgentStreamErrorInfo } from '@/common/chat/chatLib';

export type ConversationErrorReportContext = {
  error: AgentStreamErrorInfo;
  conversationId: string;
  messageId?: string;
  turnId?: string;
  occurredAt: string;
};

export function buildConversationErrorReportMetadata(context: ConversationErrorReportContext) {
  const { error } = context;
  return {
    schemaVersion: 1,
    reportType: 'conversation_error',
    source: 'error_card_feedback',
    ...(error.incident_id ? { incidentId: error.incident_id } : {}),
    error: {
      message: error.message,
      ...(error.code ? { code: error.code } : {}),
      ...(error.ownership ? { ownership: error.ownership } : {}),
      ...(error.retryable !== undefined ? { retryable: error.retryable } : {}),
      ...(error.feedback_recommended !== undefined
        ? { feedbackRecommended: error.feedback_recommended }
        : {}),
      ...(error.resolution ? { resolution: error.resolution } : {}),
    },
    correlation: {
      conversationId: context.conversationId,
      ...(context.messageId ? { messageId: context.messageId } : {}),
      ...(context.turnId ? { turnId: context.turnId } : {}),
      occurredAt: context.occurredAt,
    },
  };
}
