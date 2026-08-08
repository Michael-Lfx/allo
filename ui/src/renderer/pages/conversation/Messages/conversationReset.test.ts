import { afterEach, describe, expect, test } from 'bun:test';

import type { TMessage } from '@/common/chat/chatLib';
import type { ConversationId, MessageId } from '@/common/types/ids';

import {
  __resetConversationMessageCoordinators,
  armBarrier,
  beginEditResubmitReconciliation,
  captureBarrier,
  captureReconciliationSnapshot,
  commitAuthoritativeConversationReset,
  describeConversation,
  getEpoch,
  retainConsumer,
} from './conversationMessageCoordinator';
import { applyFetchedMessages } from './hooks';

const conversationId = '019fa2b0-6dc2-75c1-9b50-2742e02df27a' as ConversationId;
const otherConversationId = '019fa2b0-6dc2-75c1-9b50-2742e02df27b' as ConversationId;
const targetMessageId = '019fa2b0-6dc2-75c1-9b50-2742e02df2a0' as MessageId;
const oldAssistantMessageId = '019fa2b0-6dc2-75c1-9b50-2742e02df2a1' as MessageId;
const errorTipMessageId = '019fa2b0-6dc2-75c1-9b50-2742e02df2a2' as MessageId;

const textMessage = (
  id: string,
  position: 'left' | 'right',
  createdAt: number,
  messageId?: MessageId,
  content = id
): TMessage => ({
  id,
  message_id: messageId,
  msg_id: messageId,
  conversation_id: conversationId,
  type: 'text',
  position,
  created_at: createdAt,
  content: { content },
});

const errorTip = (id: string, createdAt: number, messageId?: MessageId): TMessage => ({
  id,
  message_id: messageId,
  msg_id: messageId,
  conversation_id: conversationId,
  type: 'tips',
  position: 'left',
  created_at: createdAt,
  content: { content: 'something went wrong', type: 'error' },
});

/** A typical old suffix: target user message + assistant reply + persisted error tip. */
const oldSuffix = (): TMessage[] => [
  textMessage('prefix', 'left', 90, '019fa2b0-6dc2-75c1-9b50-2742e02df290' as MessageId),
  textMessage('target', 'right', 100, targetMessageId),
  textMessage('old-assistant', 'left', 101, oldAssistantMessageId),
  errorTip('old-error-tip', 102, errorTipMessageId),
];

/** Mirrors the loadMessages apply step: discard on epoch drift, else freeze the
 * reconciliation snapshot and compose with it. */
const applyFetch = (
  currentList: TMessage[],
  fetched: TMessage[],
  capturedEpoch: number
): TMessage[] => {
  if (capturedEpoch !== getEpoch(conversationId)) return currentList;
  return applyFetchedMessages(currentList, fetched, captureReconciliationSnapshot(conversationId));
};

const ids = (list: TMessage[]): string[] => list.map((message) => message.id);

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

    const result = applyFetch(currentList, oldSuffix(), capturedEpoch);
    expect(result).toBe(currentList);
    expect(ids(result)).toEqual(ids(currentList));
  });

  test('a post-reset fetch captures the new epoch and applies normally', () => {
    retainConsumer(conversationId);
    commitAuthoritativeConversationReset(conversationId);

    const capturedEpoch = getEpoch(conversationId);
    const result = applyFetch([], oldSuffix(), capturedEpoch);
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