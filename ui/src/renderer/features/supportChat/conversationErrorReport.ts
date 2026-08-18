/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { AgentStreamErrorInfo } from '@/common/chat/chatLib';
import { buildAgentErrorDiagnostic, buildErrorDiagnostic } from '@/renderer/utils/ui/errorDiagnostics';

export type ConversationErrorReportContext = {
  error: AgentStreamErrorInfo;
  conversationId: string;
  messageId?: string;
  turnId?: string;
  occurredAt: string;
};

export function buildConversationErrorReportMetadata(context: ConversationErrorReportContext) {
  const { error } = context;
  const diagnostic = buildAgentErrorDiagnostic(error);
  const safeMessage = buildErrorDiagnostic({ message: error.message }).summary;
  const safeResolution = diagnostic.resolutionKind
    ? {
        kind: diagnostic.resolutionKind,
        ...(diagnostic.resolutionTarget ? { target: diagnostic.resolutionTarget } : {}),
      }
    : undefined;
  return {
    schemaVersion: 1,
    reportType: 'conversation_error',
    source: 'error_card_feedback',
    ...(diagnostic.incidentId ? { incidentId: diagnostic.incidentId } : {}),
    error: {
      message: safeMessage,
      ...(diagnostic.code ? { code: diagnostic.code } : {}),
      ...(diagnostic.ownership ? { ownership: diagnostic.ownership } : {}),
      ...(error.retryable !== undefined ? { retryable: error.retryable } : {}),
      ...(error.feedback_recommended !== undefined
        ? { feedbackRecommended: error.feedback_recommended }
        : {}),
      ...(diagnostic.detail ? { detail: diagnostic.detail } : {}),
      ...(safeResolution ? { resolution: safeResolution } : {}),
    },
    correlation: {
      conversationId: context.conversationId,
      ...(context.messageId ? { messageId: context.messageId } : {}),
      ...(context.turnId ? { turnId: context.turnId } : {}),
      occurredAt: context.occurredAt,
    },
  };
}
