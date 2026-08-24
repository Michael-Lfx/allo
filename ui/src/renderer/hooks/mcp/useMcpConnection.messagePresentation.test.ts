import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./useMcpConnection.ts', import.meta.url), 'utf8');

describe('MCP connection check messages', () => {
  test('uses the unified notification facade shared by model health checks', () => {
    expect(source.includes("import { AppMessage as Message } from '@/renderer/components/notifications';")).toBe(true);
    expect(source.includes("import { Message } from '@arco-design/web-react';")).toBe(false);
    expect(source.includes("import { globalMessageQueue } from './messageQueue';")).toBe(false);
    expect(source.includes('Message.warning({')).toBe(true);
    expect(source.includes('Message.success({')).toBe(true);
    expect(source.includes('Message.error({')).toBe(true);
  });
});
