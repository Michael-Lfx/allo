/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export type TurnProcessCycleGroup<T> =
  | { type: 'cycle'; items: T[] }
  | { type: 'item'; item: T };

const CYCLE_CHILD_KINDS = new Set(['text', 'tool']);

export const groupTurnProcessItemsByCycle = <T,>(
  items: readonly T[],
  getKind: (item: T) => string
): TurnProcessCycleGroup<T>[] => {
  const groups: TurnProcessCycleGroup<T>[] = [];
  let index = 0;

  while (index < items.length) {
    const item = items[index];
    if (getKind(item) !== 'thinking') {
      groups.push({ type: 'item', item });
      index += 1;
      continue;
    }

    const cycleItems: T[] = [item];
    index += 1;
    while (index < items.length) {
      const nextKind = getKind(items[index]);
      if (nextKind === 'thinking' || !CYCLE_CHILD_KINDS.has(nextKind)) break;
      cycleItems.push(items[index]);
      index += 1;
    }
    groups.push({ type: 'cycle', items: cycleItems });
  }

  return groups;
};
