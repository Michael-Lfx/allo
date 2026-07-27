/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import type { ICloudImMessage } from '@/common/adapter/ipcBridge';
import {
  createPendingMessage,
  mergeServerMessages,
  replacePendingMessage,
} from './supportMessageMerge';

function message(partial: Partial<ICloudImMessage> & Pick<ICloudImMessage, 'id' | 'seq'>): ICloudImMessage {
  return {
    conversationId: 1001,
    clientMsgId: null,
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

describe('supportMessageMerge', () => {
  test('keeps log payload on pending messages so failed uploads can be retried', () => {
    const logPayload = {
      objectKey: 'hardware/feedback/logs/20260727/log.zip',
      url: 'https://download.example/log.zip',
      name: 'log.zip',
      contentType: 'application/zip',
      byteSize: 1024,
    };

    const pending = createPendingMessage(
      'client-log',
      '附上日志，请协助排查',
      '2026-07-27T12:00:00Z',
      'sending',
      logPayload
    );

    expect(pending.logPayload).toEqual(logPayload);
  });

  test('sorts by seq, deduplicates polls, and replaces matching pending message', () => {
    const pending = createPendingMessage('client-1', 'hello', '2026-07-24T10:00:00Z');
    const server = message({ id: 9, seq: 3, clientMsgId: 'client-1', content: 'hello' });

    expect(mergeServerMessages([pending], [server])).toEqual([{ kind: 'server', message: server }]);
    expect(mergeServerMessages([{ kind: 'server', message: server }], [server])).toEqual([
      { kind: 'server', message: server },
    ]);
  });

  test('keeps unmatched pending messages after merged server history', () => {
    const older = message({ id: 1, seq: 1, content: 'first' });
    const pending = createPendingMessage('client-2', 'second', '2026-07-24T10:01:00Z');
    const merged = mergeServerMessages(
      [{ kind: 'server', message: older }, pending],
      [older]
    );
    expect(merged).toEqual([{ kind: 'server', message: older }, pending]);
  });

  test('replacePendingMessage swaps the matching pending bubble', () => {
    const pending = createPendingMessage('client-1', 'hello', '2026-07-24T10:00:00Z');
    const server = message({ id: 9, seq: 3, clientMsgId: 'client-1', content: 'hello' });
    expect(replacePendingMessage([pending], 'client-1', server)).toEqual([
      { kind: 'server', message: server },
    ]);
  });
});
