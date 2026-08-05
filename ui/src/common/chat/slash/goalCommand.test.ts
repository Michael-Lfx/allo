import { describe, expect, test } from 'bun:test';
import { parseGoalSlashCommand } from './goalCommand';

describe('goal slash command parsing', () => {
  test('treats bare /goal as composer start mode', () => {
    expect(parseGoalSlashCommand('/goal')).toEqual({ action: 'start' });
    expect(parseGoalSlashCommand('/goal   ')).toEqual({ action: 'start' });
  });

  test('keeps explicit status and goal-setting commands distinct', () => {
    expect(parseGoalSlashCommand('/goal status')).toEqual({ action: 'status' });
    expect(parseGoalSlashCommand('/goal Ship the release')).toEqual({
      action: 'set',
      objective: 'Ship the release',
    });
  });

  test('preserves goal and subgoal control commands', () => {
    expect(parseGoalSlashCommand('/goal pause')).toEqual({ action: 'pause' });
    expect(parseGoalSlashCommand('/goal resume')).toEqual({ action: 'resume' });
    expect(parseGoalSlashCommand('/goal clear')).toEqual({ action: 'clear' });
    expect(parseGoalSlashCommand('/subgoal Add tests')).toEqual({
      action: 'add_subgoal',
      subgoal: 'tests',
    });
    expect(parseGoalSlashCommand('/subgoal list')).toEqual({ action: 'list_subgoals' });
    expect(parseGoalSlashCommand('/subgoal remove 2')).toEqual({ action: 'remove_subgoal', index_1based: 2 });
    expect(parseGoalSlashCommand('/subgoal clear')).toEqual({ action: 'clear_subgoals' });
  });
});
