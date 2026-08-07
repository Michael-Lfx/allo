import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

/**
 * Structural contract for P2-1: `loadOlder` must fence on the conversation
 * epoch. A keyset page requested at epoch N that returns after an edit-resubmit
 * success (epoch N+1) would prepend pre-truncate rows back above the live tail —
 * the same resurrection class as the main fetch path, through pagination.
 */

const hooksSource = readFileSync(new URL('./hooks.ts', import.meta.url), 'utf8');

describe('loadOlder epoch fencing (P2-1)', () => {
  test('captures the epoch before the request and discards the page when it drifted', () => {
    const loadOlder = hooksSource.slice(
      hooksSource.indexOf('const loadOlder = useCallback'),
      hooksSource.indexOf('// Register this message-list instance')
    );
    const invoke = loadOlder.indexOf('ipcBridge.database.getConversationMessages.invoke');
    const capture = loadOlder.indexOf('const capturedEpoch = getEpoch(key);');
    const fence = loadOlder.indexOf('capturedEpoch !== getEpoch(key)');

    expect(invoke).toBeGreaterThan(-1);
    // Captured before the await, compared after it.
    expect(capture).toBeGreaterThan(-1);
    expect(capture).toBeLessThan(invoke);
    expect(fence).toBeGreaterThan(invoke);
    // The fence sits inside the post-await staleness guard (early return).
    const guard = loadOlder.indexOf('activeConversationRef.current !== key', invoke);
    expect(guard).toBeGreaterThan(invoke);
    expect(fence).toBeGreaterThan(guard - 120); // same guard statement
    expect(fence).toBeLessThan(guard + 120);
  });
});
