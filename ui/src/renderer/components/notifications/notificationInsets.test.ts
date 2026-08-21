import { describe, expect, test } from 'bun:test';
import { calculateNotificationBottomInset } from './notificationInsets';

describe('calculateNotificationBottomInset', () => {
  test('returns the base inset when nothing blocks the anchor', () => {
    expect(calculateNotificationBottomInset(800, [], 24)).toBe(24);
  });

  test('lifts above a blocker by its height plus the gap', () => {
    // Blocker top at 700 in an 800px viewport: 100px of blocker + 12px gap.
    expect(calculateNotificationBottomInset(800, [700], 24)).toBe(112);
  });

  test('the tallest blocker wins', () => {
    expect(calculateNotificationBottomInset(800, [750, 600, 780], 24)).toBe(212);
  });

  test('never drops below the base inset', () => {
    expect(calculateNotificationBottomInset(800, [796], 24)).toBe(24);
  });

  test('ignores blockers below the viewport fold', () => {
    expect(calculateNotificationBottomInset(800, [900], 24)).toBe(24);
  });

  test('rounds fractional results up to whole pixels', () => {
    expect(calculateNotificationBottomInset(800.5, [700.25], 16)).toBe(Math.ceil(800.5 - 700.25 + 12));
  });
});
