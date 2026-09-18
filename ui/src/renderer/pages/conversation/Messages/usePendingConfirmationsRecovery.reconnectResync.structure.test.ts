/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import { parseConversationId } from '@/common/types/ids';
import type { TMessage } from '@/common/chat/chatLib';
import { hasPermissionMessageForCallId } from './usePendingConfirmationsRecovery';

const source = readFileSync(new URL('./usePendingConfirmationsRecovery.ts', import.meta.url), 'utf8');

describe('pending confirmations reconnect recovery', () => {
  test('re-runs the recovery list fetch after websocket reconnect', () => {
    // WebSocket delivery has no replay: a confirmation raised while delivery
    // was gapped must be recovered by re-fetching the pending list.
    expect(source.includes('const recoverPendingConfirmations = ')).toBe(true);
    expect(source.includes('ipcBridge.conversation.reconnected.on')).toBe(true);
  });

  test('dedupes recovered confirmations against both legacy and native ACP cards', () => {
    const conversation_id = parseConversationId('0190f5fe-7c00-7a00-8000-000000000001');
    const legacyPermission = {
      id: 'confirmation:legacy',
      type: 'permission',
      conversation_id,
      content: {
        id: 'legacy',
        call_id: 'call-legacy',
        description: 'legacy',
        options: [],
      },
    } as TMessage;
    const acpPermission = {
      id: 'acp:live',
      type: 'acp_permission',
      conversation_id,
      content: {
        session_id: 'session-1',
        options: [],
        tool_call: { tool_call_id: 'call-acp' },
      },
    } as TMessage;

    expect(hasPermissionMessageForCallId([legacyPermission], 'call-legacy')).toBe(true);
    expect(hasPermissionMessageForCallId([acpPermission], 'call-acp')).toBe(true);
    expect(hasPermissionMessageForCallId([legacyPermission, acpPermission], 'other-call')).toBe(false);
  });
});
