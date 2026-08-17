/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import type { TMessage } from '@/common/chat/chatLib';
import { parseConversationId, parseMessageId, type ConversationId, type MessageId } from '@/common/types/ids';
import { buildTurnPreview, normalizeDisplayIndex } from './minimapUtils';

const conversationId: ConversationId = parseConversationId('0190f5fe-7c00-7a00-8000-000000000004');

const messageId = (suffix: string): MessageId =>
  parseMessageId(`0190f5fe-7c00-7a00-8000-${suffix.padStart(12, '0')}`);

const textMessage = ({
  renderId,
  msgId,
  messageId: durableId,
  content,
  hidden = false,
  position = 'right',
}: {
  renderId: string;
  msgId?: MessageId;
  messageId?: MessageId;
  content: string;
  hidden?: boolean;
  position?: 'left' | 'right';
}): TMessage =>
  ({
    id: renderId,
    msg_id: msgId,
    message_id: durableId,
    conversation_id: conversationId,
    type: 'text',
    position,
    hidden,
    content: { content },
    created_at: 1,
  }) as TMessage;

describe('buildTurnPreview', () => {
  test('normalizes an absent virtual display row to null', () => {
    expect(normalizeDisplayIndex(-1)).toBeNull();
    expect(normalizeDisplayIndex(0)).toBe(0);
  });

  test('projects only visible, non-empty, stable user questions', () => {
    const visibleId = messageId('1');
    const turns = buildTurnPreview([
      textMessage({ renderId: 'hidden', msgId: messageId('2'), content: 'hidden', hidden: true }),
      textMessage({ renderId: 'empty', msgId: messageId('3'), content: '' }),
      textMessage({ renderId: 'unstable', content: 'no stable id' }),
      textMessage({ renderId: 'assistant', msgId: messageId('4'), content: 'answer', position: 'left' }),
      textMessage({ renderId: 'visible', msgId: visibleId, content: 'visible' }),
    ]);

    expect(turns).toHaveLength(1);
    expect(turns[0]?.messageId).toBe(visibleId);
    expect(turns[0]?.questionRaw).toBe('visible');
  });

  test('dedupes one canonical identity but keeps identical content with different identities', () => {
    const firstId = messageId('10');
    const secondId = messageId('11');
    const turns = buildTurnPreview([
      textMessage({ renderId: 'first', msgId: firstId, content: 'same text' }),
      textMessage({ renderId: 'duplicate-render-row', msgId: firstId, content: 'same text' }),
      textMessage({ renderId: 'second', msgId: secondId, content: 'same text' }),
    ]);

    expect(turns).toHaveLength(2);
    expect(turns.map((turn) => turn.msgId)).toEqual([firstId, secondId]);
  });

  test('prefers durable message identity when projecting a persisted question', () => {
    const durableId = messageId('20');
    const turns = buildTurnPreview([
      textMessage({
        renderId: 'live-question',
        msgId: messageId('21'),
        messageId: durableId,
        content: 'same persisted question',
      }),
      textMessage({
        renderId: 'db-question',
        msgId: messageId('22'),
        messageId: durableId,
        content: 'same persisted question',
      }),
    ]);

    expect(turns).toHaveLength(1);
    expect(turns[0]?.messageId).toBe(durableId);
  });
});
