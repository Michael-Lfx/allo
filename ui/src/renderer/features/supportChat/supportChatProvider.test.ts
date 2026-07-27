/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import type { ICloudImConversation, ICloudImMessage } from '@/common/adapter/ipcBridge';
import { createSupportPollController } from './supportPollController';

type Scheduled = { id: number; ms: number; fn: () => void };

function createFakeTimers() {
  let nextId = 1;
  const scheduled: Scheduled[] = [];
  return {
    scheduled,
    setTimeout(fn: () => void, ms: number) {
      const id = nextId++;
      scheduled.push({ id, ms, fn });
      return id;
    },
    clearTimeout(id: unknown) {
      const index = scheduled.findIndex((item) => item.id === id);
      if (index >= 0) scheduled.splice(index, 1);
    },
    async flushNext() {
      const item = scheduled.shift();
      if (!item) throw new Error('no timer');
      item.fn();
      await Promise.resolve();
      await Promise.resolve();
    },
    peekMs() {
      return scheduled[0]?.ms;
    },
  };
}

function conversation(partial?: Partial<ICloudImConversation>): ICloudImConversation {
  return {
    id: 1,
    userId: 2,
    externalChannelCode: 'flowy',
    app: 'flowymes',
    status: 'open',
    assigneeSysUserId: null,
    lastSeq: 1,
    lastMessageId: 1,
    lastMessageAt: '2026-07-24T10:00:00Z',
    lastMessagePreview: 'hi',
    lastSenderType: 'sys_user',
    userUnreadCount: 1,
    opsUnreadCount: 0,
    hasUnread: true,
    createdAt: '2026-07-20T10:00:00Z',
    updatedAt: '2026-07-24T10:00:00Z',
    closedAt: null,
    ...partial,
  };
}

function message(seq: number): ICloudImMessage {
  return {
    id: seq,
    conversationId: 1,
    seq,
    clientMsgId: null,
    senderType: 'sys_user',
    senderId: 9,
    msgType: 'text',
    content: `m-${seq}`,
    status: 'sent',
    createdAt: '2026-07-24T10:00:00Z',
  };
}

describe('createSupportPollController', () => {
  test('never overlaps in-flight requests and uses unread/message/hidden intervals', async () => {
    const timers = createFakeTimers();
    const deferred: { resolve: ((value: ICloudImConversation) => void) | null } = { resolve: null };
    const unreadCalls: ICloudImConversation[] = [];
    const messageCalls: ICloudImMessage[][] = [];
    let conversationCalls = 0;
    let messagePollCalls = 0;

    const controller = createSupportPollController({
      getConversation: () => {
        conversationCalls += 1;
        return new Promise<ICloudImConversation>((resolve) => {
          deferred.resolve = resolve;
        });
      },
      listMessagesAfter: async (afterSeq) => {
        messagePollCalls += 1;
        return [message(afterSeq + 1)];
      },
      onUnread: (c) => unreadCalls.push(c),
      onMessages: (list) => messageCalls.push(list),
      setTimeout: timers.setTimeout,
      clearTimeout: timers.clearTimeout,
    });

    controller.start();
    expect(conversationCalls).toBe(1);
    expect(controller.isInFlight()).toBe(true);

    // Second poll while in-flight must not start another request.
    controller.pollNow();
    expect(conversationCalls).toBe(1);

    expect(deferred.resolve).not.toBeNull();
    deferred.resolve!(conversation());
    await Promise.resolve();
    await Promise.resolve();
    expect(unreadCalls).toHaveLength(1);
    expect(timers.peekMs()).toBe(15_000);

    controller.setModalOpen(true);
    await Promise.resolve();
    await Promise.resolve();
    expect(messagePollCalls).toBe(1);
    expect(messageCalls).toHaveLength(1);
    expect(timers.peekMs()).toBe(3_000);

    controller.setVisibility('hidden');
    expect(timers.peekMs()).toBe(60_000);

    controller.dispose();
    const pendingBefore = timers.scheduled.length;
    await timers.flushNext().catch(() => undefined);
    expect(timers.scheduled.length).toBeLessThanOrEqual(pendingBefore);
    expect(conversationCalls).toBe(1);
  });

  test('backs off 15/30/60 on failures and recovers after success', async () => {
    const timers = createFakeTimers();
    let shouldFail = true;
    const controller = createSupportPollController({
      getConversation: async () => {
        if (shouldFail) throw new Error('network');
        return conversation({ userUnreadCount: 0, hasUnread: false });
      },
      listMessagesAfter: async () => [],
      onUnread: () => undefined,
      onMessages: () => undefined,
      setTimeout: timers.setTimeout,
      clearTimeout: timers.clearTimeout,
    });

    controller.start();
    await Promise.resolve();
    await Promise.resolve();
    expect(timers.peekMs()).toBe(15_000);

    await timers.flushNext();
    expect(timers.peekMs()).toBe(30_000);

    await timers.flushNext();
    expect(timers.peekMs()).toBe(60_000);

    shouldFail = false;
    await timers.flushNext();
    expect(timers.peekMs()).toBe(15_000);

    controller.dispose();
  });

  test('dispose prevents further polls', async () => {
    const timers = createFakeTimers();
    let calls = 0;
    const controller = createSupportPollController({
      getConversation: async () => {
        calls += 1;
        return conversation();
      },
      listMessagesAfter: async () => [],
      onUnread: () => undefined,
      onMessages: () => undefined,
      setTimeout: timers.setTimeout,
      clearTimeout: timers.clearTimeout,
    });

    controller.start();
    await Promise.resolve();
    await Promise.resolve();
    expect(calls).toBe(1);
    controller.dispose();
    expect(timers.scheduled).toHaveLength(0);
    controller.pollNow();
    expect(calls).toBe(1);
  });
});
