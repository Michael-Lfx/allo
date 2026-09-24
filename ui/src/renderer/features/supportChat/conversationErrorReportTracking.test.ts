/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { beforeEach, describe, expect, test } from 'bun:test';
import {
  getErrorReportRecord,
  isErrorFreshForAutoReport,
  markAutoReportFailed,
  markAutoReportInFlight,
  markAutoReportSuccess,
  recordUserSubmissionSuccess,
  resetErrorReportTrackingForTests,
  shouldDeduplicateUserSubmission,
} from './conversationErrorReportTracking';

describe('conversation error report tracking & deduplication', () => {
  const contextKey = 'conv-1:turn-1:2026-09-24T00:00:00.000Z';

  beforeEach(() => {
    resetErrorReportTrackingForTests();
  });

  test('marks in-flight only once and denies concurrent duplicate auto reports', () => {
    expect(markAutoReportInFlight(contextKey)).toBe(true);
    expect(markAutoReportInFlight(contextKey)).toBe(false);

    markAutoReportSuccess(contextKey, 12345);
    expect(markAutoReportInFlight(contextKey)).toBe(false);

    const record = getErrorReportRecord(contextKey);
    expect(record?.autoStatus).toBe('reported');
    expect(record?.autoReportedAt).toBe(12345);
  });

  test('allows retry if previous auto report failed', () => {
    expect(markAutoReportInFlight(contextKey)).toBe(true);
    markAutoReportFailed(contextKey);

    expect(getErrorReportRecord(contextKey)?.autoStatus).toBe('failed');
    expect(markAutoReportInFlight(contextKey)).toBe(true);
  });

  test('does not deduplicate when error was never auto reported or failed', () => {
    expect(
      shouldDeduplicateUserSubmission(contextKey, { description: '', screenshotsCount: 0 })
    ).toBe(false);

    markAutoReportInFlight(contextKey);
    markAutoReportFailed(contextKey);

    expect(
      shouldDeduplicateUserSubmission(contextKey, { description: '', screenshotsCount: 0 })
    ).toBe(false);
  });

  test('deduplicates blank user submission when auto report succeeded or is in-flight', () => {
    markAutoReportInFlight(contextKey);

    // While in-flight, blank submission is deduplicated
    expect(
      shouldDeduplicateUserSubmission(contextKey, { description: '  ', screenshotsCount: 0 })
    ).toBe(true);

    markAutoReportSuccess(contextKey);

    // After success, blank submission is deduplicated
    expect(
      shouldDeduplicateUserSubmission(contextKey, { description: '', screenshotsCount: 0 })
    ).toBe(true);
  });

  test('does not deduplicate when user provides supplementary description or screenshots', () => {
    markAutoReportInFlight(contextKey);
    markAutoReportSuccess(contextKey);

    // User provided non-empty description
    expect(
      shouldDeduplicateUserSubmission(contextKey, {
        description: 'Failed when clicking search',
        screenshotsCount: 0,
      })
    ).toBe(false);

    // User provided screenshots
    expect(
      shouldDeduplicateUserSubmission(contextKey, {
        description: '',
        screenshotsCount: 1,
      })
    ).toBe(false);
  });

  test('deduplicates identical user submission on repeated clicks', () => {
    markAutoReportInFlight(contextKey);
    markAutoReportSuccess(contextKey);

    const draft = { description: 'Specific error description', screenshotsCount: 2 };
    expect(shouldDeduplicateUserSubmission(contextKey, draft)).toBe(false);

    recordUserSubmissionSuccess(contextKey, draft);

    // Re-submitting the exact same content is deduplicated
    expect(shouldDeduplicateUserSubmission(contextKey, draft)).toBe(true);
    expect(
      shouldDeduplicateUserSubmission(contextKey, {
        description: '   Specific error description   ',
        screenshotsCount: 2,
      })
    ).toBe(true);

    // Changing description is not deduplicated
    expect(
      shouldDeduplicateUserSubmission(contextKey, {
        description: 'Updated error description',
        screenshotsCount: 2,
      })
    ).toBe(false);

    // Changing screenshots count is not deduplicated
    expect(
      shouldDeduplicateUserSubmission(contextKey, {
        description: 'Specific error description',
        screenshotsCount: 3,
      })
    ).toBe(false);
  });

  test('detects screenshot replacement with the same count using fingerprints', () => {
    markAutoReportInFlight(contextKey);
    markAutoReportSuccess(contextKey);

    const screenshotA = { fileName: 'shotA.png', file: new Blob(['a'], { type: 'image/png' }) };
    const screenshotB = { fileName: 'shotB.png', file: new Blob(['bb'], { type: 'image/png' }) };

    const initialDraft = { description: 'Report', screenshots: [screenshotA] };
    expect(shouldDeduplicateUserSubmission(contextKey, initialDraft)).toBe(false);

    recordUserSubmissionSuccess(contextKey, initialDraft);

    // Submitting with exact same screenshot is deduplicated
    expect(shouldDeduplicateUserSubmission(contextKey, initialDraft)).toBe(true);

    // Replacing with a different screenshot of the same count is NOT deduplicated
    const replacedDraft = { description: 'Report', screenshots: [screenshotB] };
    expect(shouldDeduplicateUserSubmission(contextKey, replacedDraft)).toBe(false);
  });

  test('historical errors beyond the freshness window are not auto-reported', () => {
    const now = Date.now();
    const freshOccurredAt = new Date(now - 2 * 60 * 1000).toISOString(); // 2 mins ago
    const oldOccurredAt = new Date(now - 15 * 60 * 1000).toISOString(); // 15 mins ago

    expect(isErrorFreshForAutoReport(freshOccurredAt, now)).toBe(true);
    expect(isErrorFreshForAutoReport(oldOccurredAt, now)).toBe(false);

    const freshKey = 'conv-1:fresh:2026';
    const oldKey = 'conv-1:old:2026';

    expect(markAutoReportInFlight(freshKey, freshOccurredAt)).toBe(true);
    expect(markAutoReportInFlight(oldKey, oldOccurredAt)).toBe(false);
  });
});
