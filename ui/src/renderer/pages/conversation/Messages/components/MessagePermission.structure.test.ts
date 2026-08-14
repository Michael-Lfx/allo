import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./MessagePermission.tsx', import.meta.url), 'utf8');

describe('MessagePermission Approval Card adapter', () => {
  test('renders the Beautiful UI approval shell without changing confirmation IPC', () => {
    expect(source.includes('<ApprovalCard')).toBe(true);
    expect(source.includes('kindFromPermissionAction')).toBe(true);
    expect(source.includes("data-testid='message-permission-card'")).toBe(true);
    expect(source.includes('ipcBridge.conversation.confirmation.confirm.invoke')).toBe(true);
  });
});
