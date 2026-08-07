import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, test } from 'bun:test';

/**
 * Anti-drift guard for the two refresh channels (K):
 *
 *  - 'chat.history.refresh' → sidebar conversation list ONLY; its sole listener
 *    lives in SessionList/hooks/useConversationListSync.ts.
 *  - 'conversation.messages.refresh' → message transcript reload ONLY; its sole
 *    listener lives in Messages/hooks.ts (useMessageLstCache).
 *
 * If a second listener ever appears for either channel, fetches double up and
 * the single-consumer reasoning (epoch fencing, ack semantics) stops holding —
 * fail here instead of rediscovering it as a race.
 */

const rendererRoot = fileURLToPath(new URL('../../..', import.meta.url));

const collectSources = (dir: string): string[] => {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      out.push(...collectSources(full));
    } else if (/\.(ts|tsx)$/.test(entry) && !/\.test\.(ts|tsx)$/.test(entry)) {
      out.push(full);
    }
  }
  return out;
};

const listenersOf = (event: string): string[] =>
  collectSources(rendererRoot)
    .filter((file) => {
      const source = readFileSync(file, 'utf8');
      return (
        source.includes(`addEventListener('${event}'`) ||
        source.includes(`useAddEventListener('${event}'`)
      );
    })
    .map((file) => relative(rendererRoot, file).replace(/\\/g, '/'))
    .sort();

describe('refresh channel single-consumer contract (K)', () => {
  test("'chat.history.refresh' is listened to only by the sidebar list sync", () => {
    expect(listenersOf('chat.history.refresh')).toEqual([
      'pages/conversation/SessionList/hooks/useConversationListSync.ts',
    ]);
  });

  test("'conversation.messages.refresh' is listened to only by the message-list cache", () => {
    expect(listenersOf('conversation.messages.refresh')).toEqual([
      'pages/conversation/Messages/hooks.ts',
    ]);
  });
});
