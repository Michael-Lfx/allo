import type { TMessage } from '@/common/chat/chatLib';
import type { ConversationId, MessageId } from '@/common/types/ids';

import { captureReconciliationSnapshot, getEpoch } from './conversationMessageCoordinator';
import { applyFetchedMessages } from './hooks';

export const conversationId = '019fa2b0-6dc2-75c1-9b50-2742e02df27a' as ConversationId;
export const otherConversationId = '019fa2b0-6dc2-75c1-9b50-2742e02df27b' as ConversationId;
export const targetMessageId = '019fa2b0-6dc2-75c1-9b50-2742e02df2a0' as MessageId;
export const oldAssistantMessageId = '019fa2b0-6dc2-75c1-9b50-2742e02df2a1' as MessageId;
export const errorTipMessageId = '019fa2b0-6dc2-75c1-9b50-2742e02df2a2' as MessageId;

export const textMessage = (
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

export const errorTip = (id: string, createdAt: number, messageId?: MessageId): TMessage => ({
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
export const oldSuffix = (): TMessage[] => [
  textMessage('prefix', 'left', 90, '019fa2b0-6dc2-75c1-9b50-2742e02df290' as MessageId),
  textMessage('target', 'right', 100, targetMessageId),
  textMessage('old-assistant', 'left', 101, oldAssistantMessageId),
  errorTip('old-error-tip', 102, errorTipMessageId),
];

/** Mirrors the loadMessages apply step: discard on epoch drift, else capture
 * the reconciliation snapshot and compose with it (the snapshot freeze happens
 * at acceptance time, before the updater would run — exactly like
 * mergeIntoList). */
export const applyFetch = (
  currentList: TMessage[],
  fetched: TMessage[],
  conversationId: ConversationId,
  capturedEpoch: number
): TMessage[] => {
  if (capturedEpoch !== getEpoch(conversationId)) return currentList;
  return applyFetchedMessages(currentList, fetched, captureReconciliationSnapshot(conversationId));
};

export const ids = (list: TMessage[]): string[] => list.map((message) => message.id);