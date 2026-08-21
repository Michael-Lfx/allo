

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('useGuidSend pending preset guard', () => {
  test('blocks send and disables the button until the preset catalog resolves', () => {
    const source = readSource(new URL('./useGuidSend.ts', import.meta.url));

    expect(source.includes('if (preset_id && (!agentInfo || agentInfo.preset_id !== preset_id))')).toBe(true);
    expect(source.includes('is_presetAgentPending && !resolvedPresetSelection')).toBe(true);
  });

  test('preflights a missing model before loading or beginning the pending overlay', () => {
    const source = readSource(new URL('./useGuidSend.ts', import.meta.url));
    const handler = source.slice(source.indexOf('const sendMessageHandler'), source.indexOf('// Calculate button'));
    const gate = handler.indexOf("readinessBlocker === 'model' || needsModelBeforeSend");

    expect(gate).toBeGreaterThan(-1);
    expect(gate).toBeLessThan(handler.indexOf('setLoading(true)'));
    expect(gate).toBeLessThan(handler.indexOf('beginPending?.'));
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

  test('carries the selected real workspace into the pending title header', () => {
    const source = readSource(new URL('./useGuidSend.ts', import.meta.url));
    const overlay = readSource(
      new URL('../../conversation/components/ConversationShell/PendingConversationOverlay.tsx', import.meta.url)
    );

    expect(source.includes('workspacePath: dir.trim() || undefined')).toBe(true);
    expect(overlay.includes('pending.workspacePath')).toBe(true);
    expect(overlay.includes('getWorkspaceTitleSubtitle(pending.workspacePath, isMobile)')).toBe(true);
    expect(overlay.includes('CHAT_HEADER_WITH_SUBTITLE_CLASSES')).toBe(true);
    expect(overlay.includes('marqueeOnHover')).toBe(false);
  });

  test('arms the reveal handshake only after navigation; aborts instantly on failure', () => {
    const source = readSource(new URL('./useGuidSend.ts', import.meta.url));

    // handleSend reports whether navigate was dispatched; the overlay's
    // teardown path is chosen from that outcome.
    expect(source.includes('handleSend: () => Promise<boolean>')).toBe(true);
    expect(source.includes('attachPending?.(')).toBe(true);

    const handler = source.slice(source.indexOf('const sendMessageHandler'), source.indexOf('// Calculate button'));
    expect(handler.includes('abortPending?.()')).toBe(true);
    expect(handler.includes('endPending?.()')).toBe(true);

    // endPending must not live in .finally — an aborted transition would wait
    // out the reveal timeout instead of dropping instantly. (Slice only the
    // finally callback body; the useCallback deps array legitimately mentions
    // endPending.)
    const finallyStart = handler.indexOf('.finally(');
    const finallyBody = handler.slice(finallyStart, handler.indexOf('});', finallyStart));
    expect(finallyBody.includes('endPending')).toBe(false);
  });
});
