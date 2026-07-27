/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import type { ICloudImConversation, ICloudImMessage } from '@/common/adapter/ipcBridge';
import { createPendingMessage } from './supportMessageMerge';
import { initialSupportChatState, supportChatReducer } from './supportChatReducer';

function conversation(partial?: Partial<ICloudImConversation>): ICloudImConversation {
  return {
    id: 1001,
    userId: 20001,
    externalChannelCode: 'flowy',
    app: 'flowymes',
    status: 'open',
    assigneeSysUserId: null,
    lastSeq: 3,
    lastMessageId: 9,
    lastMessageAt: '2026-07-24T10:00:00Z',
    lastMessagePreview: 'hello',
    lastSenderType: 'user',
    userUnreadCount: 0,
    opsUnreadCount: 0,
    hasUnread: false,
    createdAt: '2026-07-20T10:00:00Z',
    updatedAt: '2026-07-24T10:00:00Z',
    closedAt: null,
    ...partial,
  };
}

function serverMessage(partial?: Partial<ICloudImMessage>): ICloudImMessage {
  return {
    id: 9,
    conversationId: 1001,
    seq: 3,
    clientMsgId: 'client-1',
    senderType: 'user',
    senderId: 1,
    msgType: 'text',
    content: 'hello',
    status: 'sent',
    createdAt: '2026-07-24T10:00:00Z',
    duplicate: false,
    ...partial,
  };
}

describe('supportChatReducer', () => {
  test('opens into loading and closes back to closed while preserving unread', () => {
    const withUnread = supportChatReducer(initialSupportChatState, {
      type: 'set-unread',
      unreadCount: 2,
    });
    const loading = supportChatReducer(withUnread, { type: 'open' });
    expect(loading).toEqual({ status: 'loading', unreadCount: 2 });
    expect(supportChatReducer(loading, { type: 'close' })).toEqual({
      status: 'closed',
      unreadCount: 2,
    });
  });

  test('ready state tracks conversation, messages, and pending delivery updates', () => {
    const pending = createPendingMessage('client-1', 'hello', '2026-07-24T10:00:00Z');
    const ready = supportChatReducer({ status: 'loading', unreadCount: 1 }, {
      type: 'ready',
      conversation: conversation({ userUnreadCount: 0 }),
      messages: [pending],
    });
    expect(ready.status).toBe('ready');
    if (ready.status !== 'ready') throw new Error('expected ready');

    const failed = supportChatReducer(ready, { type: 'pending-failed', clientMsgId: 'client-1' });
    expect(failed.status).toBe('ready');
    if (failed.status !== 'ready') throw new Error('expected ready');
    expect(failed.messages[0]).toEqual({ ...pending, delivery: 'failed' });

    const replaced = supportChatReducer(failed, {
      type: 'pending-replaced',
      clientMsgId: 'client-1',
      message: serverMessage(),
    });
    expect(replaced.status).toBe('ready');
    if (replaced.status !== 'ready') throw new Error('expected ready');
    expect(replaced.messages).toEqual([{ kind: 'server', message: serverMessage() }]);
  });

  test('auth-required and error preserve unread count', () => {
    const base = supportChatReducer(initialSupportChatState, {
      type: 'set-unread',
      unreadCount: 4,
    });
    expect(supportChatReducer(base, { type: 'auth-required' })).toEqual({
      status: 'auth-required',
      unreadCount: 4,
    });
    expect(
      supportChatReducer(base, { type: 'error', message: 'temporarily unavailable' })
    ).toEqual({
      status: 'error',
      unreadCount: 4,
      message: 'temporarily unavailable',
    });
  });
});
