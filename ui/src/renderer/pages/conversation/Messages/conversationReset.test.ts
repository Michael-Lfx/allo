import { afterEach, describe, expect, test } from 'bun:test';

import {
  __resetConversationMessageCoordinators,
  armBarrier,
  beginEditResubmitReconciliation,
  captureBarrier,
  commitAuthoritativeConversationReset,
  describeConversation,
  getEpoch,
  retainConsumer,
} from './conversationMessageCoordinator';
import {
  applyFetch,
  conversationId,
  ids,
  oldSuffix,
  otherConversationId,
  targetMessageId,
} from './conversationTestData';

afterEach(() => {
  __resetConversationMessageCoordinators();
});

describe('commitAuthoritativeConversationReset', () => {
  test('bumps the epoch and clears every barrier while consumers are retained', () => {
    const a = retainConsumer(conversationId);
    const b = retainConsumer(conversationId);
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-armed', capture);
    armBarrier(conversationId, 'op-reconciling', capture);
    beginEditResubmitReconciliation(conversationId, 'op-reconciling');

    const beforeEpoch = getEpoch(conversationId);
    expect(beforeEpoch).toBeGreaterThan(0);
    expect(describeConversation(conversationId).barriers.length).toBe(2);

    commitAuthoritativeConversationReset(conversationId);

    const after = describeConversation(conversationId);
    expect(getEpoch(conversationId)).toBe(beforeEpoch + 1);
    expect(after.barriers).toEqual([]);
    expect(after.consumerCount).toBe(2);
    expect(a.consumerId).not.toBe(b.consumerId);
  });

  test('a pre-reset fetch captured at the old epoch is discarded', () => {
    retainConsumer(conversationId);
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-1', capture);
    beginEditResubmitReconciliation(conversationId, 'op-1');

    const currentList = oldSuffix();
    const capturedEpoch = getEpoch(conversationId);
    commitAuthoritativeConversationReset(conversationId);

    const result = applyFetch(currentList, oldSuffix(), conversationId, capturedEpoch);
    expect(result).toBe(currentList);
    expect(ids(result)).toEqual(ids(currentList));
  });

  test('a post-reset fetch captures the new epoch and applies normally', () => {
    retainConsumer(conversationId);
    commitAuthoritativeConversationReset(conversationId);

    const capturedEpoch = getEpoch(conversationId);
    const result = applyFetch([], oldSuffix(), conversationId, capturedEpoch);
    expect(result.length).toBeGreaterThan(0);
    expect(ids(result)).toContain('target');
    expect(ids(result)).toContain('old-error-tip');
  });

  test('unknown conversation is a no-op and never leaks a coordinator', () => {
    commitAuthoritativeConversationReset(otherConversationId);
    const after = describeConversation(otherConversationId);
    expect(after.epoch).toBe(0);
    expect(after.consumerCount).toBe(0);
    expect(after.barriers).toEqual([]);
  });

  test('without consumers the reset lets the coordinator be destroyed', () => {
    const capture = captureBarrier(oldSuffix(), targetMessageId, 100)!;
    armBarrier(conversationId, 'op-armed', capture);
    expect(describeConversation(conversationId).barriers.length).toBe(1);

    commitAuthoritativeConversationReset(conversationId);

    const after = describeConversation(conversationId);
    expect(after.epoch).toBe(0);
    expect(after.consumerCount).toBe(0);
    expect(after.barriers).toEqual([]);
  });
});