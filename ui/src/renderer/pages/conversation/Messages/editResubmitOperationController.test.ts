import { describe, expect, test } from 'bun:test';

import { parseConversationId, parseMessageId } from '@/common/types/ids';

import {
  __resetEditResubmitOperations,
  beginEditResubmitOperation,
  claimEditResubmitRunner,
  getEditResubmitOperation,
  releaseEditResubmitOperation,
  releaseEditResubmitRunner,
  subscribeRecoverableEditResubmitOperation,
  updateEditResubmitOperation,
} from './editResubmitOperationController';
import {
  __resetEditingMessageStore,
  clearEditingMessageByOperation,
  getEditingMessage,
  setEditingMessage,
} from './editingMessageStore';

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

  test('notifies a waiting remount when the previous renderer releases its runner', async () => {
    __resetEditResubmitOperations();
    beginEditResubmitOperation({ ...record('op-a'), phase: 'confirming' });
    claimEditResubmitRunner(conversationId, 'op-a', 'renderer-a');
    const available: string[] = [];
    const unsubscribe = subscribeRecoverableEditResubmitOperation(
      conversationId,
      (operation) => available.push(operation.operationId)
    );

    expect(available).toEqual([]);
    releaseEditResubmitRunner(conversationId, 'op-a', 'renderer-a');
    await Promise.resolve();
    expect(available).toEqual(['op-a']);

    unsubscribe();
  });

  test('lets the originating renderer claim before recovery adoption in the same tick', async () => {
    __resetEditResubmitOperations();
    const available: string[] = [];
    const unsubscribe = subscribeRecoverableEditResubmitOperation(
      conversationId,
      (operation) => available.push(operation.operationId)
    );

    beginEditResubmitOperation(record('op-a'));
    expect(claimEditResubmitRunner(conversationId, 'op-a', 'originating-renderer')).toBe(true);
    expect(available).toEqual([]);
    await Promise.resolve();
    expect(available).toEqual([]);

    releaseEditResubmitRunner(conversationId, 'op-a', 'originating-renderer');
    await Promise.resolve();
    expect(available).toEqual(['op-a']);
    unsubscribe();
  });

  test('does not auto-adopt an unsubmitted editing operation', () => {
    __resetEditResubmitOperations();
    beginEditResubmitOperation({ ...record('op-a'), phase: 'editing' });
    let calls = 0;
    const unsubscribe = subscribeRecoverableEditResubmitOperation(
      conversationId,
      () => {
        calls += 1;
      }
    );
    expect(calls).toBe(0);
    unsubscribe();
  });

  test('cancels a scheduled recovery adoption when the subscriber unmounts', async () => {
    __resetEditResubmitOperations();
    let calls = 0;
    const unsubscribe = subscribeRecoverableEditResubmitOperation(
      conversationId,
      () => {
        calls += 1;
      }
    );
    beginEditResubmitOperation(record('op-a'));
    unsubscribe();
    await Promise.resolve();
    expect(calls).toBe(0);
  });

  test('terminal cleanup leaves a clean draft before one new-key retry is admitted', () => {
    __resetEditResubmitOperations();
    __resetEditingMessageStore();
    beginEditResubmitOperation(record('op-edit'));
    setEditingMessage(conversationId, {
      ownerId: 'renderer-a',
      msgId: targetMessageId,
      pending: true,
      phase: 'confirming',
      operationId: 'op-edit',
    });

    clearEditingMessageByOperation(conversationId, 'op-edit');
    releaseEditResubmitOperation(conversationId, 'op-edit');
    expect(getEditingMessage(conversationId)).toBeUndefined();
    expect(getEditResubmitOperation(conversationId)).toBeUndefined();

    expect(beginEditResubmitOperation({ ...record('op-retry'), source: 'retry' })).toBe(true);
    expect(beginEditResubmitOperation({ ...record('op-duplicate'), source: 'retry' })).toBe(false);
    expect(getEditResubmitOperation(conversationId)?.operationId).toBe('op-retry');
  });
});
