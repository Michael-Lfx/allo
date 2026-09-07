import { describe, expect, test } from 'bun:test';
import { BackendHttpError, BackendRequestError } from '@/common/adapter/httpBridge';
import {
  classifyConversationSendFailure,
  isAmbiguousConversationDeliveryError,
  isConversationTurnAdmissionConflict,
} from './conversationSendRecovery';

const admissionConflict = () =>
  new BackendHttpError({
    method: 'POST',
    path: '/api/conversations/test/messages',
    status: 409,
    body: {
      success: false,
      error: 'Conversation lifecycle rejected durable turn admission',
      code: 'CONFLICT',
      details: {
        kind: 'conversation_turn_admission',
        retryable: true,
        reconcile: 'conversation.get',
      },
    },
  });

describe('conversation send recovery classification', () => {
  test('recognizes only the structured lifecycle admission conflict', () => {
    expect(isConversationTurnAdmissionConflict(admissionConflict())).toBe(true);
    expect(
      isConversationTurnAdmissionConflict(
        new BackendHttpError({
          method: 'POST',
          path: '/api/conversations/test/messages',
          status: 409,
          body: {
            success: false,
            error: 'Conversation lifecycle rejected durable turn admission',
            code: 'CONFLICT',
          },
        })
      )
    ).toBe(false);
  });

  test('treats only response-ambiguous failures as same-key transport recovery', () => {
    expect(isAmbiguousConversationDeliveryError(new BackendRequestError('network', 'lost'))).toBe(true);
    expect(isAmbiguousConversationDeliveryError(new BackendRequestError('timeout', 'timed out'))).toBe(true);
    expect(isAmbiguousConversationDeliveryError(new BackendRequestError('aborted', 'cancelled'))).toBe(false);
    expect(
      isAmbiguousConversationDeliveryError(
        new BackendHttpError({ method: 'POST', path: '/messages', status: 503, body: {} })
      )
    ).toBe(true);
    expect(
      isAmbiguousConversationDeliveryError(
        new BackendHttpError({ method: 'POST', path: '/messages', status: 400, body: {} })
      )
    ).toBe(false);
  });

  test('keeps the three failure classes distinct', () => {
    expect(classifyConversationSendFailure(admissionConflict())).toBe('turn_admission_conflict');
    expect(classifyConversationSendFailure(new BackendRequestError('network', 'lost'))).toBe('ambiguous_transport');
    expect(classifyConversationSendFailure(new Error('validation failed'))).toBe('terminal');
  });
});
