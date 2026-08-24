import { describe, expect, test } from 'bun:test';
import React from 'react';
import {
  getCollapsedRecords,
  getCollapseExitKeys,
  mergeCollapsedRecordsWithExits,
  sortByCreatedAt,
  textFromNode,
  MAX_VISIBLE_TRANSIENT,
} from './notificationStackModel';
import type { StoredNotification } from './notificationTypes';

let sequence = 0;
const notice = (overrides: Partial<StoredNotification> = {}): StoredNotification => {
  sequence += 1;
  return {
    key: `key-${sequence}`,
    scopeId: 'scope-test',
    level: 'info',
    content: `content-${sequence}`,
    duration: 3000,
    remainingMs: 3000,
    closable: false,
    showIcon: true,
    passthrough: false,
    createdAt: sequence,
    revision: 0,
    status: 'active',
    ...overrides,
  };
};

describe('getCollapsedRecords', () => {
  test('shows the newest 3 transients and counts the hidden rest', () => {
    const records = [notice(), notice(), notice(), notice()];
    const collapsed = getCollapsedRecords(records);
    expect(collapsed.records.map((item) => item.key)).toEqual(['key-2', 'key-3', 'key-4']);
    expect(collapsed.hiddenCount).toBe(1);
    expect(collapsed.scrollable).toBe(false);
  });

  test('shows everything at exactly the limit', () => {
    const collapsed = getCollapsedRecords([notice(), notice(), notice()]);
    expect(collapsed.records).toHaveLength(MAX_VISIBLE_TRANSIENT);
    expect(collapsed.hiddenCount).toBe(0);
  });

  test('persistent notifications stay visible and displace transient slots', () => {
    const persistentA = notice({ duration: 0, remainingMs: 0 });
    const persistentB = notice({ duration: 0, remainingMs: 0 });
    const transientA = notice();
    const transientB = notice();
    const transientC = notice();
    const collapsed = getCollapsedRecords([persistentA, persistentB, transientA, transientB, transientC]);
    expect(collapsed.records.map((item) => item.key)).toEqual([persistentA.key, persistentB.key, transientC.key]);
    expect(collapsed.hiddenCount).toBe(2);
    expect(collapsed.scrollable).toBe(false);
  });

  test('more persistent notifications than the limit switch to scrollable and hide all transients', () => {
    const records = [
      notice({ duration: 0, remainingMs: 0 }),
      notice({ duration: 0, remainingMs: 0 }),
      notice({ duration: 0, remainingMs: 0 }),
      notice({ duration: 0, remainingMs: 0 }),
      notice(),
      notice(),
    ];
    const collapsed = getCollapsedRecords(records);
    expect(collapsed.records).toHaveLength(4);
    expect(collapsed.records.every((item) => item.duration === 0)).toBe(true);
    expect(collapsed.scrollable).toBe(true);
    expect(collapsed.hiddenCount).toBe(2);
  });

  test('exiting records keep their slot but never count as hidden', () => {
    // 5 transients: the two oldest are out of view; one of them is exiting.
    const records = [
      notice({ status: 'exiting' }),
      notice({ status: 'exiting' }),
      notice(),
      notice(),
      notice(),
    ];
    const collapsed = getCollapsedRecords(records);
    // Hidden pair: key-1 (exiting) and key-2 (exiting) — only actives count.
    expect(collapsed.hiddenCount).toBe(0);
    // A visible exiting card still occupies its slot: of these 4 transients
    // the newest 3 show (including the exiting one), the oldest active hides.
    const stayingA = notice();
    const exitingVisible = notice({ status: 'exiting' });
    const stayingB = notice();
    const newest = notice();
    const second = getCollapsedRecords([stayingA, exitingVisible, stayingB, newest]);
    expect(second.records.map((item) => item.key)).toEqual([exitingVisible.key, stayingB.key, newest.key]);
    expect(second.hiddenCount).toBe(1);
  });

  test('orders output by createdAt across the persistent/transient merge', () => {
    const late = notice({ duration: 0, remainingMs: 0 });
    const records = [notice(), notice(), late, notice()];
    const collapsed = getCollapsedRecords(records);
    const createdAts = collapsed.records.map((item) => item.createdAt);
    expect(createdAts).toEqual([...createdAts].sort((a, b) => a - b));
    expect(collapsed.records.some((item) => item.key === late.key)).toBe(true);
  });
});

describe('sortByCreatedAt / textFromNode', () => {
  test('sortByCreatedAt sorts a copy without mutating the input', () => {
    const a = notice();
    const b = notice();
    const input = [b, a];
    expect(sortByCreatedAt(input).map((item) => item.key)).toEqual([a.key, b.key]);
    expect(input[0]).toBe(b);
  });

  test('textFromNode flattens strings, numbers, arrays and elements', () => {
    expect(textFromNode('plain')).toBe('plain');
    expect(textFromNode(42)).toBe('42');
    expect(textFromNode(['a', null, 'b'])).toBe('a b');
    expect(textFromNode(React.createElement('div', null, 'x', 'y'))).toBe('x y');
    expect(textFromNode(undefined)).toBe('');
  });
});

describe('collapse transitions', () => {
  test('identifies records hidden by the collapsed projection in their prior order', () => {
    const records = [notice(), notice(), notice(), notice()];
    const collapsed = getCollapsedRecords(records);
    expect(getCollapseExitKeys(records, collapsed.records)).toEqual([records[0].key]);
  });

  test('merges still-present exit records without duplicating visible records', () => {
    const records = [notice(), notice(), notice(), notice()];
    const exitKeys = new Set([records[0].key]);
    const merged = mergeCollapsedRecordsWithExits(records, exitKeys);
    expect(merged.map((item) => item.key)).toEqual(records.map((item) => item.key));
  });
});
