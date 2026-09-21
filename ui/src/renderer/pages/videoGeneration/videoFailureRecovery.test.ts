import { describe, expect, test } from 'bun:test';
import type { FailureKind } from './classifyFailure';
import { resolveVideoFailureRecoveryAction } from './videoFailureRecovery';

describe('video failure recovery action', () => {
  test('always opens billing for insufficient-credit failures', () => {
    expect(resolveVideoFailureRecoveryAction('credits')).toEqual({
      labelKey: 'billing.openBilling',
      source: 'open_billing',
    });
  });

  test('does not create a billing action for other failure kinds', () => {
    const kinds: FailureKind[] = ['llm', 'image', 'video', 'moderation', 'unknown'];
    for (const kind of kinds) {
      expect(resolveVideoFailureRecoveryAction(kind)).toBeNull();
    }
    expect(resolveVideoFailureRecoveryAction(undefined)).toBeNull();
  });
});
