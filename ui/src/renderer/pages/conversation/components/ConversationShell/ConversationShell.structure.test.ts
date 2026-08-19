import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, test } from 'bun:test';

const readLocalSource = (fileName: string) =>
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), fileName), 'utf8');

describe('ConversationShell Guid launch', () => {
  test('covers the Guid-to-conversation jump so the empty prompt does not flash', () => {
    const source = readLocalSource('index.tsx');

    expect(source.includes('<PendingConversationOverlay />')).toBe(true);
  });

  test('does not present the launch cover as a multi-step preparation page', () => {
    const overlay = readLocalSource('PendingConversationOverlay.tsx');

    expect(overlay.includes('conversation.pending.stepValidate')).toBe(false);
    expect(overlay.includes('conversation.pending.stepCreate')).toBe(false);
    expect(overlay.includes('conversation.pending.stepConfigure')).toBe(false);
    expect(overlay.includes('conversation.pending.stepOpen')).toBe(false);
    expect(overlay.includes('pendingSteps')).toBe(false);
  });
});
