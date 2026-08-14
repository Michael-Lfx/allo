import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./MessageToolGroup.tsx', import.meta.url), 'utf8');

describe('MessageToolGroup ConfirmationDetails Approval Card adapter', () => {
  test('wraps confirming details in the Beautiful UI card without dropping confirm IPC', () => {
    expect(source.includes('<ApprovalCard')).toBe(true);
    expect(source.includes('kindFromConfirmationType(confirmationDetails.type)')).toBe(true);
    expect(source.includes('ipcBridge.conversation.confirmMessage')).toBe(true);
    expect(source.includes('<EditConfirmationDiff')).toBe(true);
  });
});
