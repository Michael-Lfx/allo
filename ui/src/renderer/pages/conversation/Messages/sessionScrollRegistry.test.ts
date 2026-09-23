/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { beforeEach, describe, expect, test } from 'bun:test';
import { parseConversationId } from '@/common/types/ids';
import { emitter } from '@/renderer/utils/emitter';
import { sessionScrollRegistry } from './sessionScrollRegistry';

describe('sessionScrollRegistry', () => {
  beforeEach(() => {
    sessionScrollRegistry.clearAll();
  });

  test('saves and retrieves scroll snapshot for a conversation', () => {
    sessionScrollRegistry.save('conv-1', {
      scrollTop: 450,
      userScrolled: true,
    });

    const snapshot = sessionScrollRegistry.get('conv-1');
    expect(snapshot).toBeDefined();
    expect(snapshot?.scrollTop).toBe(450);
    expect(snapshot?.userScrolled).toBe(true);
    expect(snapshot?.updatedAt).toBeGreaterThan(0);
  });

  test('returns undefined for unknown or empty conversation id', () => {
    expect(sessionScrollRegistry.get('non-existent')).toBeUndefined();
    expect(sessionScrollRegistry.get('')).toBeUndefined();
    expect(sessionScrollRegistry.get(undefined)).toBeUndefined();
    expect(sessionScrollRegistry.get(null)).toBeUndefined();
  });

  test('ignores saving with empty or falsy conversation id', () => {
    sessionScrollRegistry.save('', { scrollTop: 100, userScrolled: false });
    sessionScrollRegistry.save(undefined, { scrollTop: 100, userScrolled: false });
    sessionScrollRegistry.save(null, { scrollTop: 100, userScrolled: false });
    expect(sessionScrollRegistry.size()).toBe(0);
  });

  test('clamps negative scrollTop to 0', () => {
    sessionScrollRegistry.save('conv-neg', {
      scrollTop: -50,
      userScrolled: false,
    });
    expect(sessionScrollRegistry.get('conv-neg')?.scrollTop).toBe(0);
  });

  test('clears specific conversation or all conversations', () => {
    sessionScrollRegistry.save('conv-1', { scrollTop: 100, userScrolled: false });
    sessionScrollRegistry.save('conv-2', { scrollTop: 200, userScrolled: true });
    expect(sessionScrollRegistry.size()).toBe(2);

    sessionScrollRegistry.clear('conv-1');
    expect(sessionScrollRegistry.get('conv-1')).toBeUndefined();
    expect(sessionScrollRegistry.get('conv-2')).toBeDefined();
    expect(sessionScrollRegistry.size()).toBe(1);

    sessionScrollRegistry.clearAll();
    expect(sessionScrollRegistry.size()).toBe(0);
  });

  test('enforces LRU eviction up to 200 sessions', () => {
    for (let i = 0; i < 205; i += 1) {
      sessionScrollRegistry.save(`conv-${i}`, {
        scrollTop: i * 10,
        userScrolled: i % 2 === 0,
      });
    }

    expect(sessionScrollRegistry.size()).toBe(200);
    // The first 5 items (conv-0 to conv-4) should have been evicted
    expect(sessionScrollRegistry.get('conv-0')).toBeUndefined();
    expect(sessionScrollRegistry.get('conv-1')).toBeUndefined();
    expect(sessionScrollRegistry.get('conv-4')).toBeUndefined();
    // conv-5 and later should exist
    expect(sessionScrollRegistry.get('conv-5')).toBeDefined();
    expect(sessionScrollRegistry.get('conv-204')).toBeDefined();
  });

  test('refreshes LRU recency on access so accessed items survive eviction', () => {
    sessionScrollRegistry.save('conv-initial', { scrollTop: 10, userScrolled: false });
    for (let i = 0; i < 199; i += 1) {
      sessionScrollRegistry.save(`conv-fill-${i}`, { scrollTop: 20, userScrolled: false });
    }
    expect(sessionScrollRegistry.size()).toBe(200);

    // Access conv-initial to make it most recently used
    expect(sessionScrollRegistry.get('conv-initial')).toBeDefined();

    // Add another item: should evict conv-fill-0, not conv-initial
    sessionScrollRegistry.save('conv-new', { scrollTop: 30, userScrolled: false });
    expect(sessionScrollRegistry.get('conv-initial')).toBeDefined();
    expect(sessionScrollRegistry.get('conv-fill-0')).toBeUndefined();
  });

  test('clears session scroll snapshot when conversation.deleted event is emitted', () => {
    const testId = '019b0000-0000-7000-8000-000000000999';
    sessionScrollRegistry.save(testId, { scrollTop: 300, userScrolled: true });
    expect(sessionScrollRegistry.get(testId)).toBeDefined();

    emitter.emit('conversation.deleted', parseConversationId(testId));
    expect(sessionScrollRegistry.get(testId)).toBeUndefined();
  });
});
