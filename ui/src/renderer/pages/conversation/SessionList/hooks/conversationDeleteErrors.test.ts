import { describe, expect, it } from 'vitest';

import {
  classifyConversationDeleteError,
  conversationDeleteMessageKey,
  summarizeConversationDeleteResults,
} from './conversationDeleteErrors';

function backendError(code: string) {
  return {
    name: 'BackendHttpError',
    status: code === 'CONVERSATION_DELETE_PENDING' ? 502 : 409,
    code,
  };
}

describe('conversation deletion errors', () => {
  it('classifies stable backend deletion codes without parsing messages', () => {
    expect(classifyConversationDeleteError(backendError('CONVERSATION_ATTEMPT_RETAINED'))).toBe(
      'attemptRetained',
    );
    expect(classifyConversationDeleteError(backendError('CONVERSATION_RUNNING_ORPHAN'))).toBe(
      'runningOrphan',
    );
    expect(classifyConversationDeleteError(backendError('CONVERSATION_DELETE_PENDING'))).toBe(
      'pending',
    );
    expect(classifyConversationDeleteError(backendError('CONFLICT'))).toBe('unknown');
    expect(classifyConversationDeleteError(new Error('Execution attempt conversations are retained'))).toBe(
      'unknown',
    );
  });

  it('selects localized single and batch message keys', () => {
    expect(conversationDeleteMessageKey('attemptRetained', false)).toBe(
      'conversation.history.deleteAttemptRetained',
    );
    expect(conversationDeleteMessageKey('runningOrphan', false)).toBe(
      'conversation.history.deleteRunningOrphan',
    );
    expect(conversationDeleteMessageKey('pending', false)).toBe('conversation.history.deletePending');
    expect(conversationDeleteMessageKey('unknown', true)).toBe(
      'conversation.history.batchDeleteFailed',
    );
  });

  it('summarizes mixed batch outcomes without hiding successful deletions', () => {
    const retainedError = backendError('CONVERSATION_ATTEMPT_RETAINED');
    const summary = summarizeConversationDeleteResults([
      { status: 'fulfilled', value: true },
      { status: 'fulfilled', value: false },
      { status: 'rejected', reason: retainedError },
      { status: 'rejected', reason: new Error('network failure') },
    ]);

    expect(summary.successCount).toBe(1);
    expect(summary.failureCounts.get('attemptRetained')).toBe(1);
    expect(summary.failureCounts.get('unknown')).toBe(2);
    expect(summary.failures).toHaveLength(3);
    expect(summary.failures[0]).toEqual({ kind: 'unknown', reason: undefined });
    expect(summary.failures[1]).toEqual({ kind: 'attemptRetained', reason: retainedError });
    expect(summary.failures[2]?.kind).toBe('unknown');
    expect(summary.failures[2]?.reason instanceof Error).toBe(true);
  });
});
