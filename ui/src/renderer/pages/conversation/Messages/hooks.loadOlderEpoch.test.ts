import { describe, expect, test } from 'bun:test';

import type { TMessage } from '@/common/chat/chatLib';
import { parseConversationId, parseMessageId } from '@/common/types/ids';

import { captureReconciliationSnapshot } from './conversationMessageCoordinator';
import { applyOlderKeysetPage } from './hooks';

const conversationId = parseConversationId('0190f5fe-7c00-7a00-8000-000000000921');
const row = (id: string, createdAt: number): TMessage => ({
  id,
  message_id: parseMessageId(id),
  msg_id: parseMessageId(id),
  conversation_id: conversationId,
  type: 'text',
  position: 'right',
  content: { content: id },
  created_at: createdAt,
});

describe('applyOlderKeysetPage', () => {
  test('accepts only rows strictly earlier than the current oldest tuple', () => {
    const current = [row('0190f5fe-7c00-7a00-8000-000000000923', 20)];
    const older = [
      row('0190f5fe-7c00-7a00-8000-000000000922', 20),
      row('0190f5fe-7c00-7a00-8000-000000000924', 20),
      row('0190f5fe-7c00-7a00-8000-000000000920', 10),
    ];
    const applied = applyOlderKeysetPage(
      current,
      older,
      0,
      0,
      captureReconciliationSnapshot(conversationId)
    );
    expect(applied.map((message) => message.id)).toEqual([
      '0190f5fe-7c00-7a00-8000-000000000922',
      '0190f5fe-7c00-7a00-8000-000000000920',
      '0190f5fe-7c00-7a00-8000-000000000923',
    ]);
  });

  test('a deferred updater cannot restore a pre-truncate suffix after epoch drift', () => {
    const current = [row('0190f5fe-7c00-7a00-8000-000000000923', 20)];
    const stale = [row('0190f5fe-7c00-7a00-8000-000000000920', 10)];
    expect(
      applyOlderKeysetPage(
        current,
        stale,
        4,
        5,
        captureReconciliationSnapshot(conversationId)
      )
    ).toBe(current);
  });
});
