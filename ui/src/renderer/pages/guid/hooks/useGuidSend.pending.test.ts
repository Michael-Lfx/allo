/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('useGuidSend pending preset guard', () => {
  test('blocks send and disables the button until the preset catalog resolves', () => {
    const source = readSource(new URL('./useGuidSend.ts', import.meta.url));

    expect(source.includes('if (is_preset && !agentInfo)')).toBe(true);
    expect(source.includes('is_presetAgentPending && !resolvedPresetSelection')).toBe(true);
  });

  test('preflights a missing model before loading or beginning the pending overlay', () => {
    const source = readSource(new URL('./useGuidSend.ts', import.meta.url));
    const handler = source.slice(source.indexOf('const sendMessageHandler'), source.indexOf('// Calculate button'));

    expect(handler.indexOf('if (needsModelBeforeSend)')).toBeGreaterThan(-1);
    expect(handler.indexOf('if (needsModelBeforeSend)')).toBeLessThan(handler.indexOf('setLoading(true)'));
    expect(handler.indexOf('if (needsModelBeforeSend)')).toBeLessThan(handler.indexOf('beginPending?.'));
  });

  test('advances pending progress from real create milestones', () => {
    const source = readSource(new URL('./useGuidSend.ts', import.meta.url));
    const overlay = readSource(
      new URL('../../conversation/components/ConversationShell/PendingConversationOverlay.tsx', import.meta.url)
    );

    expect(source.includes("advancePending?.('creating')")).toBe(true);
    expect(source.includes("advancePending?.('configuring')")).toBe(true);
    expect(source.includes("advancePending?.('opening')")).toBe(true);
    expect(overlay.includes('setInterval')).toBe(false);
  });
});
