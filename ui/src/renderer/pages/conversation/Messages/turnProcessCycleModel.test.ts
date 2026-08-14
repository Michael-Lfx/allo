/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { groupTurnProcessItemsByCycle } from './turnProcessCycleModel';

const kindOf = (item: { kind: string }) => item.kind;

describe('groupTurnProcessItemsByCycle', () => {
  test('nests text and tools under the preceding thinking header', () => {
    expect(
      groupTurnProcessItemsByCycle(
        [
          { id: 't1', kind: 'thinking' },
          { id: 'p1', kind: 'text' },
          { id: 'g1', kind: 'tool' },
          { id: 'g2', kind: 'tool' },
          { id: 't2', kind: 'thinking' },
          { id: 'p2', kind: 'text' },
        ],
        kindOf
      )
    ).toEqual([
      {
        type: 'cycle',
        items: [
          { id: 't1', kind: 'thinking' },
          { id: 'p1', kind: 'text' },
          { id: 'g1', kind: 'tool' },
          { id: 'g2', kind: 'tool' },
        ],
      },
      {
        type: 'cycle',
        items: [
          { id: 't2', kind: 'thinking' },
          { id: 'p2', kind: 'text' },
        ],
      },
    ]);
  });

  test('leaves permission and status rows outside a thinking cycle', () => {
    expect(
      groupTurnProcessItemsByCycle(
        [
          { id: 't1', kind: 'thinking' },
          { id: 'g1', kind: 'tool' },
          { id: 'perm', kind: 'permission' },
          { id: 'orphan', kind: 'tool' },
        ],
        kindOf
      )
    ).toEqual([
      {
        type: 'cycle',
        items: [
          { id: 't1', kind: 'thinking' },
          { id: 'g1', kind: 'tool' },
        ],
      },
      { type: 'item', item: { id: 'perm', kind: 'permission' } },
      { type: 'item', item: { id: 'orphan', kind: 'tool' } },
    ]);
  });

  test('keeps a thinking header without followers as its own cycle', () => {
    expect(groupTurnProcessItemsByCycle([{ id: 't1', kind: 'thinking' }], kindOf)).toEqual([
      { type: 'cycle', items: [{ id: 't1', kind: 'thinking' }] },
    ]);
  });
});
