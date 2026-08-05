import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = () => readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');

describe('GoalModeChip armed mode', () => {
  test('shows and cancels the temporary goal mode without clearing a persisted goal', () => {
    const source = readSource();

    expect(source.includes('armed?: boolean')).toBe(true);
    expect(source.includes('onArmedChange?: (armed: boolean) => void')).toBe(true);
    expect(source.includes('if (!armed && !hasActiveGoal)')).toBe(true);
    expect(source.includes('const clearsPersistedGoal = hasActiveGoal && !armed')).toBe(true);
    expect(source.includes('onClick={clearsPersistedGoal ? undefined : () => onArmedChange?.(false)}')).toBe(true);
    expect(source.includes('if (!clearsPersistedGoal)')).toBe(true);
    expect(source.includes("goalCommand.run({ action: 'clear' })")).toBe(true);
  });
});
