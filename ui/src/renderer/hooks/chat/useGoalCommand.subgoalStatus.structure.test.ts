import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = () => readFileSync(new URL('./useGoalCommand.ts', import.meta.url), 'utf8');

describe('subgoal status without a goal', () => {
  test('shows the subgoal-specific prerequisite guidance', () => {
    const source = readSource();
    const listSubgoals = source.indexOf("case 'list_subgoals':");
    const prerequisiteMessage = source.indexOf("t('conversation.goal.toast.subgoalRequiresGoal')");

    expect(listSubgoals).toBeGreaterThan(-1);
    expect(prerequisiteMessage).toBeGreaterThan(listSubgoals);
  });
});
