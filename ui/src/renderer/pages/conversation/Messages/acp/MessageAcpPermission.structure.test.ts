import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./MessageAcpPermission.tsx', import.meta.url), 'utf8');

describe('MessageAcpPermission Approval Card adapter', () => {
  test('renders the Beautiful UI approval shell without changing confirmation IPC', () => {
    expect(source.includes('<ApprovalCard')).toBe(true);
    expect(source.includes("data-testid='message-acp-permission-card'")).toBe(true);
    expect(source.includes('conversation.confirmMessage.invoke')).toBe(true);
  });
});
