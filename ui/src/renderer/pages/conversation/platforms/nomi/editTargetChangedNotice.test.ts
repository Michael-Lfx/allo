import { describe, expect, test } from 'bun:test';

import { resolveEditTargetChangedNotice } from './editTargetChangedNotice';

describe('resolveEditTargetChangedNotice', () => {
  test('shows only the authoritative stale-target outcome', () => {
    expect(resolveEditTargetChangedNotice(false, 'target_changed')).toBe(true);
  });

  test.each(['dismissed', 'operation_started', 'conversation_changed'] as const)(
    'clears the notice on %s',
    (event) => {
      expect(resolveEditTargetChangedNotice(true, event)).toBe(false);
    }
  );
});
