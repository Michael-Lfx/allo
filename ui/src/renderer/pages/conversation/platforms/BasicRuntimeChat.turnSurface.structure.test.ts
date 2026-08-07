/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('BasicRuntimeChat turn surface wiring', () => {
  test('passes hook turn state into ConversationProvider and BasicRuntimeTurnProvider', () => {
    const source = readSource(new URL('./BasicRuntimeChat.tsx', import.meta.url));

    expect(source.includes('const turnSurface = useConversationResponseMessages(conversation_id, {')).toBe(true);
    expect(source.includes("stream: type === 'openclaw-gateway' ? 'openclaw' : 'conversation'")).toBe(true);
    expect(source.includes('isProcessing: turnSurface.isProcessing')).toBe(true);
    expect(source.includes('activeTurnId: turnSurface.activeTurnId')).toBe(true);
    expect(source.includes('activeRequestMessageId: turnSurface.activeRequestMessageId')).toBe(true);
    expect(source.includes('<BasicRuntimeTurnProvider value={turnSurface}>')).toBe(true);
    expect(source.includes('useMessageLstCache(conversation_id, { windowed: true })')).toBe(true);
  });

  test('BasicRuntimeSendBox syncs setAiProcessing with shared turn surface', () => {
    const source = readSource(new URL('./BasicRuntimeSendBox.tsx', import.meta.url));

    expect(source.includes('useBasicRuntimeTurnSurface')).toBe(true);
    expect(source.includes('const syncSharedAiProcessing = sharedTurnSurface?.setAiProcessing')).toBe(true);
    expect(source.includes('syncSharedAiProcessing?.(value)')).toBe(true);
    expect(source.includes('[syncSharedAiProcessing]')).toBe(true);
    expect(source.includes('[sharedTurnSurface]')).toBe(false);
  });
});
