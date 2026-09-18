import { describe, expect, test } from 'bun:test';
import { isSupportSessionCurrent } from './supportSession';

describe('support session guard', () => {
  const expected = { generation: 3, accountId: 'account-a' };

  test('accepts the same authenticated account and generation', () => {
    expect(isSupportSessionCurrent(expected, expected, 'authenticated')).toBe(true);
  });

  test('rejects account switches, generation changes, and unauthenticated state', () => {
    expect(
      isSupportSessionCurrent(expected, { generation: 3, accountId: 'account-b' }, 'authenticated')
    ).toBe(false);
    expect(
      isSupportSessionCurrent(expected, { generation: 4, accountId: 'account-a' }, 'authenticated')
    ).toBe(false);
    expect(isSupportSessionCurrent(expected, expected, 'unauthenticated')).toBe(false);
  });
});
