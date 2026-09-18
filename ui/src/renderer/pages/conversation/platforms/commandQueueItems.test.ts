import { describe, expect, test } from 'bun:test';
import {
  promoteQueuedCommand,
  reorderQueuedCommand,
} from './commandQueueItems';

const item = (id: string) => ({
  id,
  input: id,
  files: [] as string[],
  created_at: 0,
});

describe('promoteQueuedCommand', () => {
  test('moves a later item to the front and leaves others in relative order', () => {
    expect(promoteQueuedCommand([item('a'), item('b'), item('c')], 'c')).toEqual([
      item('c'),
      item('a'),
      item('b'),
    ]);
  });

  test('is a no-op when the item is already first or missing', () => {
    const items = [item('a'), item('b')];
    expect(promoteQueuedCommand(items, 'a')).toBe(items);
    expect(promoteQueuedCommand(items, 'missing')).toBe(items);
  });
});

describe('reorderQueuedCommand', () => {
  test('moves the active item to the over index', () => {
    expect(reorderQueuedCommand([item('a'), item('b'), item('c')], 'c', 'a')).toEqual([
      item('c'),
      item('a'),
      item('b'),
    ]);
  });
});
