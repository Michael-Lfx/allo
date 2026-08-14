import { describe, expect, test } from 'bun:test';
import {
  chipDetailOmittingCommand,
  isCommandToolName,
  resolveToolChipStatus,
  resolveToolChipStatusFromAcp,
  resolveToolChipStatusFromProcessState,
  resolveToolChipStatusFromToolGroup,
} from './toolChipModel';

describe('resolveToolChipStatus', () => {
  test('keeps normalized tool statuses on the chip shell', () => {
    expect(resolveToolChipStatus({ status: 'pending' })).toBe('pending');
    expect(resolveToolChipStatus({ status: 'running' })).toBe('running');
    expect(resolveToolChipStatus({ status: 'completed' })).toBe('completed');
    expect(resolveToolChipStatus({ status: 'error' })).toBe('error');
    expect(resolveToolChipStatus({ status: 'canceled' })).toBe('canceled');
  });

  test('covers skipped and invalid-argument outcomes without new message types', () => {
    expect(resolveToolChipStatus({ status: 'pending', skipped: true })).toBe('skipped');
    expect(
      resolveToolChipStatus({ status: 'error', notExecutedReason: 'invalid_arguments' })
    ).toBe('invalid_arguments');
    expect(
      resolveToolChipStatus({
        status: 'error',
        skipped: true,
        notExecutedReason: 'invalid_arguments',
      })
    ).toBe('invalid_arguments');
  });

  test('maps tool-group display statuses onto the same chip shell', () => {
    expect(resolveToolChipStatusFromToolGroup('Success')).toBe('completed');
    expect(resolveToolChipStatusFromToolGroup('Executing')).toBe('running');
    expect(resolveToolChipStatusFromToolGroup('Confirming')).toBe('pending');
    expect(resolveToolChipStatusFromToolGroup('Error')).toBe('error');
    expect(resolveToolChipStatusFromToolGroup('Canceled')).toBe('canceled');
  });

  test('maps process-trail and ACP statuses onto the same chip shell', () => {
    expect(resolveToolChipStatusFromProcessState({ state: 'running' })).toBe('running');
    expect(resolveToolChipStatusFromProcessState({ state: 'waiting' })).toBe('pending');
    expect(resolveToolChipStatusFromProcessState({ state: 'completed' })).toBe('completed');
    expect(resolveToolChipStatusFromProcessState({ state: 'failed' })).toBe('error');
    expect(resolveToolChipStatusFromProcessState({ state: 'canceled' })).toBe('canceled');
    expect(resolveToolChipStatusFromProcessState({ state: 'failed', skipped: true })).toBe('skipped');
    expect(
      resolveToolChipStatusFromProcessState({
        state: 'completed',
        notExecutedReason: 'invalid_arguments',
      })
    ).toBe('invalid_arguments');
    expect(resolveToolChipStatusFromAcp('pending')).toBe('pending');
    expect(resolveToolChipStatusFromAcp('in_progress')).toBe('running');
    expect(resolveToolChipStatusFromAcp('completed')).toBe('completed');
    expect(resolveToolChipStatusFromAcp('failed')).toBe('error');
    expect(resolveToolChipStatusFromAcp('canceled')).toBe('canceled');
  });
});

describe('chipDetailOmittingCommand', () => {
  test('keeps short file and search details on the chip', () => {
    expect(isCommandToolName('Read')).toBe(false);
    expect(chipDetailOmittingCommand('Read', 'AGENTS.md')).toBe('AGENTS.md');
    expect(chipDetailOmittingCommand('Grep', 'beautifulUi')).toBe('beautifulUi');
  });

  test('hides bash and run_commands payloads so the chip stays compact', () => {
    expect(isCommandToolName('bash')).toBe(true);
    expect(chipDetailOmittingCommand('bash', 'cd C:/code/flowy/allo; git status')).toBeUndefined();
    expect(
      chipDetailOmittingCommand('Read', 'still a path', 'run_commands')
    ).toBeUndefined();
  });
});
