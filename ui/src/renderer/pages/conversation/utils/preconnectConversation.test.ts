import { describe, expect, test } from 'bun:test';
import type { ConversationId } from '@/common/types/ids';
import { PRECONNECT_INTERVAL_MS, preconnectConversation } from './preconnectConversation';

describe('preconnectConversation', () => {
  test('throttles per conversation and swallows failures', async () => {
    const calls: string[] = [];
    const preconnect = (id: ConversationId) => {
      calls.push(id);
      return Promise.reject(new Error('offline'));
    };
    const a = 'conv-preconnect-a' as ConversationId;
    const b = 'conv-preconnect-b' as ConversationId;

    expect(preconnectConversation(a, 1_000, preconnect)).toBe(true);
    expect(preconnectConversation(a, 1_000 + PRECONNECT_INTERVAL_MS - 1, preconnect)).toBe(false);
    expect(preconnectConversation(b, 1_000, preconnect)).toBe(true);
    expect(preconnectConversation(a, 1_000 + PRECONNECT_INTERVAL_MS, preconnect)).toBe(true);
    await Promise.resolve();

    expect(calls).toEqual([a, b, a]);
  });
});
