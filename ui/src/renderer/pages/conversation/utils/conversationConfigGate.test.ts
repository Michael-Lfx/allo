import { describe, expect, test } from 'bun:test';
import type { ConversationId } from '@/common/types/ids';
import { awaitConversationConfig, registerConversationConfig } from './conversationConfigGate';

const ID = 'conv-gate-test' as ConversationId;

describe('conversationConfigGate', () => {
  test('await resolves immediately when nothing is registered', async () => {
    const result = await awaitConversationConfig('never-registered' as ConversationId);
    expect(result).toBeUndefined();
  });

  test('await waits for the registered work to settle', async () => {
    const events: string[] = [];
    registerConversationConfig(
      ID,
      new Promise<void>((resolve) => {
        setTimeout(() => {
          events.push('work');
          resolve();
        }, 10);
      })
    );

    await awaitConversationConfig(ID);
    events.push('await');
    expect(events).toEqual(['work', 'await']);
  });

  test('a settled gate is forgotten — later awaits do not re-wait', async () => {
    const key = 'conv-gate-settled' as ConversationId;
    registerConversationConfig(key, Promise.resolve());
    await awaitConversationConfig(key);
    // A second await after settle resolves immediately (no hang, no stale entry).
    const result = await awaitConversationConfig(key);
    expect(result).toBeUndefined();
  });

  test('rejecting work never rejects the gate await', async () => {
    const key = 'conv-gate-reject' as ConversationId;
    registerConversationConfig(key, Promise.reject(new Error('boom')));
    let rejected = false;
    await awaitConversationConfig(key).catch(() => {
      rejected = true;
    });
    expect(rejected).toBe(false);
  });
});
