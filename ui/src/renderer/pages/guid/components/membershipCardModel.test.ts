import { describe, expect, test } from 'bun:test';
import { formatMembershipDate, membershipDisplayName, shortPlanBadge } from './membershipCardModel';

describe('shortPlanBadge', () => {
  test('strips an English plan suffix and a raw plan code', () => {
    expect(shortPlanBadge('Pro Plan', 'ProPlan')).toBe('Pro');
    expect(shortPlanBadge('Free Plan')).toBe('Free');
    expect(shortPlanBadge(null, 'FreePlan')).toBe('Free');
    expect(shortPlanBadge('Team')).toBe('Team');
  });

  test('strips Chinese plan suffixes and stays empty without a plan', () => {
    expect(shortPlanBadge('专业版计划')).toBe('专业版');
    expect(shortPlanBadge('专业版套餐')).toBe('专业版');
    expect(shortPlanBadge('  ', '  ')).toBe('');
    expect(shortPlanBadge(null, null)).toBe('');
  });
});

describe('membershipDisplayName', () => {
  test('uses the mailbox name when the account label is an email', () => {
    expect(membershipDisplayName('282578874@qq.com')).toBe('282578874');
    expect(membershipDisplayName('cacaca')).toBe('cacaca');
    expect(membershipDisplayName('  ')).toBe('');
  });
});

describe('formatMembershipDate', () => {
  test('pads the local calendar day', () => {
    expect(formatMembershipDate(new Date(2026, 9, 10))).toBe('2026.10.10');
    expect(formatMembershipDate(new Date(2026, 0, 3))).toBe('2026.01.03');
  });
});
