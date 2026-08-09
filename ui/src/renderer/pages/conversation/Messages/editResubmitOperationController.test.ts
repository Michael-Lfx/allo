import { describe, expect, test } from 'bun:test';

import { parseConversationId, parseMessageId } from '@/common/types/ids';

import {
  __resetEditResubmitOperations,
  beginEditResubmitOperation,
  claimEditResubmitRunner,
  getEditResubmitOperation,
  releaseEditResubmitRunner,
  updateEditResubmitOperation,
} from './editResubmitOperationController';

const conversationId = parseConversationId('0190f5fe-7c00-7a00-8000-000000000911');
const targetMessageId = parseMessageId('0190f5fe-7c00-7a00-8000-000000000912');

const record = (operationId: string) => ({
  conversationId,
  operationId,
  targetMessageId,
  targetCreatedAt: 100,
  originalContent: 'edited',
  attachmentPaths: ['a.png'],
  draftRevision: 4,
  source: 'edit' as const,
  phase: 'submitting' as const,
});

describe('editResubmitOperationController', () => {
  test('admits exactly one edit or retry synchronously per conversation', () => {
    __resetEditResubmitOperations();
    expect(beginEditResubmitOperation(record('op-a'))).toBe(true);
    expect(beginEditResubmitOperation({ ...record('op-b'), source: 'retry' })).toBe(false);
    expect(getEditResubmitOperation(conversationId)?.operationId).toBe('op-a');
  });

  test('preserves the same key and payload while runner ownership moves across remount', () => {
    __resetEditResubmitOperations();
    beginEditResubmitOperation(record('op-a'));
    expect(claimEditResubmitRunner(conversationId, 'op-a', 'renderer-a')).toBe(true);
    expect(claimEditResubmitRunner(conversationId, 'op-a', 'renderer-b')).toBe(false);
    releaseEditResubmitRunner(conversationId, 'op-a', 'renderer-a');
    expect(claimEditResubmitRunner(conversationId, 'op-a', 'renderer-b')).toBe(true);
    updateEditResubmitOperation(conversationId, 'op-a', {
      phase: 'confirming',
      backendInput: 'exact payload',
    });
    expect(getEditResubmitOperation(conversationId)).toMatchObject({
      operationId: 'op-a',
      phase: 'confirming',
      backendInput: 'exact payload',
      attachmentPaths: ['a.png'],
    });
  });
});
