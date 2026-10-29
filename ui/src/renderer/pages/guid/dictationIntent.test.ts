import { beforeEach, describe, expect, test } from 'bun:test';
import {
  consumeDictationIntent,
  readDictationIntent,
  requestDictationIntent,
  revokeDictationIntent,
  subscribeDictationIntent,
} from './dictationIntent';

describe('route-owned dictation intent', () => {
  beforeEach(() => {
    const pending = readDictationIntent('/');
    if (pending) revokeDictationIntent(pending.id);
    const other = readDictationIntent('/settings');
    if (other) revokeDictationIntent(other.id);
  });

  test('is consumed exactly once by the target route', () => {
    const intent = requestDictationIntent('/');
    expect(readDictationIntent('/settings')).toBeNull();
    expect(readDictationIntent('/')?.id).toBe(intent.id);
    expect(consumeDictationIntent('/settings', intent.id)).toBe(false);
    expect(consumeDictationIntent('/', intent.id)).toBe(true);
    expect(consumeDictationIntent('/', intent.id)).toBe(false);
    expect(readDictationIntent('/')).toBeNull();
  });

  test('leaving the target route revokes an unconsumed intent', () => {
    const intent = requestDictationIntent('/');
    revokeDictationIntent(intent.id);
    expect(readDictationIntent('/')).toBeNull();
  });

  test('an expired intent cannot be read or consumed', () => {
    const intent = requestDictationIntent('/');
    const original = Date.now;
    const base = original();
    Date.now = () => base + 16_000;
    try {
      expect(readDictationIntent('/')).toBeNull();
      expect(consumeDictationIntent('/', intent.id)).toBe(false);
    } finally {
      Date.now = original;
    }
    revokeDictationIntent(intent.id);
  });

  test('a newer request replaces an older one without duplicate consumption', () => {
    const first = requestDictationIntent('/');
    const second = requestDictationIntent('/');
    expect(second.id).not.toBe(first.id);
    expect(consumeDictationIntent('/', first.id)).toBe(false);
    expect(consumeDictationIntent('/', second.id)).toBe(true);
  });

  test('subscribers observe request and consumption, and unsubscribe cleanly', () => {
    const seen: Array<string | null> = [];
    const unsubscribe = subscribeDictationIntent((intent) => seen.push(intent?.id ?? null));
    const intent = requestDictationIntent('/');
    consumeDictationIntent('/', intent.id);
    unsubscribe();
    requestDictationIntent('/');
    expect(seen).toEqual([null, intent.id, null]);
    const leftover = readDictationIntent('/');
    if (leftover) revokeDictationIntent(leftover.id);
  });
});
