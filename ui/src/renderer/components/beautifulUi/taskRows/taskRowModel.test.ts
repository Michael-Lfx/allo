import { describe, expect, test } from 'bun:test';
import { resolveTaskGroupStatus, resolveTaskRowStatusFromProcessState } from './taskRowModel';

describe('resolveTaskRowStatusFromProcessState', () => {
  test('covers Flowy process states including waiting and canceled', () => {
    expect(resolveTaskRowStatusFromProcessState('running')).toBe('running');
    expect(resolveTaskRowStatusFromProcessState('waiting')).toBe('waiting');
    expect(resolveTaskRowStatusFromProcessState('completed')).toBe('completed');
    expect(resolveTaskRowStatusFromProcessState('failed')).toBe('failed');
    expect(resolveTaskRowStatusFromProcessState('canceled')).toBe('canceled');
  });

  test('keeps the disclosure header lifecycle-only so failed turns still read as completed', () => {
    expect(resolveTaskGroupStatus('running')).toBe('running');
    expect(resolveTaskGroupStatus('waiting')).toBe('waiting');
    expect(resolveTaskGroupStatus('completed')).toBe('completed');
    expect(resolveTaskGroupStatus('failed')).toBe('completed');
    expect(resolveTaskGroupStatus('canceled')).toBe('canceled');
  });
});
