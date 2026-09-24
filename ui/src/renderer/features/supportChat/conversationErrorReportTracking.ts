/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export type ErrorReportAutoStatus = 'idle' | 'in_flight' | 'reported' | 'failed';

export type ErrorReportRecord = {
  contextKey: string;
  autoStatus: ErrorReportAutoStatus;
  autoReportedAt?: number;
  lastSubmittedDescription?: string;
  lastSubmittedScreenshotsCount?: number;
  lastSubmittedScreenshotsFingerprint?: string;
  supplementCount: number;
};

export type UserSubmissionDraftLike = {
  description: string;
  screenshotsCount?: number;
  screenshots?: Array<{ fileName?: string; file?: Blob }>;
};

const MAX_TRACKED_ERROR_RECORDS = 100;
const STORAGE_KEY = 'flowy:reported_error_records.v1';
export const AUTO_REPORT_FRESHNESS_WINDOW_MS = 10 * 60 * 1000; // 10 minutes

let persistenceInitialized = false;
const reportRecords = new Map<string, ErrorReportRecord>();

function initPersistenceIfNeeded(): void {
  if (persistenceInitialized) return;
  persistenceInitialized = true;
  try {
    if (typeof window === 'undefined' || !window.localStorage) return;
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return;
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) {
      for (const item of parsed) {
        if (item && typeof item.contextKey === 'string') {
          reportRecords.set(item.contextKey, item);
        }
      }
    }
  } catch {
    // Ignore storage parse failure
  }
}

function persistRecords(): void {
  try {
    if (typeof window === 'undefined' || !window.localStorage) return;
    const serialized = Array.from(reportRecords.values())
      .filter((r) => r.autoStatus === 'reported')
      .slice(-MAX_TRACKED_ERROR_RECORDS);
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(serialized));
  } catch {
    // Ignore storage write failure
  }
}

function buildScreenshotsFingerprint(
  screenshots?: Array<{ fileName?: string; file?: Blob }>,
  fallbackCount: number = 0
): { count: number; fingerprint: string } {
  if (!screenshots) {
    return {
      count: fallbackCount,
      fingerprint: `count:${fallbackCount}`,
    };
  }
  const items = screenshots.map((item) => {
    const name = item.fileName ?? (item.file instanceof File ? item.file.name : '');
    const size = item.file?.size ?? 0;
    return `${name}:${size}`;
  });
  items.sort();
  return {
    count: screenshots.length,
    fingerprint: items.join(';'),
  };
}

function ensureRecord(contextKey: string): ErrorReportRecord {
  initPersistenceIfNeeded();
  let record = reportRecords.get(contextKey);
  if (!record) {
    if (reportRecords.size >= MAX_TRACKED_ERROR_RECORDS) {
      const oldestKey = reportRecords.keys().next().value;
      if (oldestKey) {
        reportRecords.delete(oldestKey);
      }
    }
    record = {
      contextKey,
      autoStatus: 'idle',
      supplementCount: 0,
    };
    reportRecords.set(contextKey, record);
  }
  return record;
}

export function isErrorFreshForAutoReport(occurredAt?: string, now = Date.now()): boolean {
  if (!occurredAt) return true;
  const time = Date.parse(occurredAt);
  if (!Number.isFinite(time)) return true;
  return now - time <= AUTO_REPORT_FRESHNESS_WINDOW_MS;
}

export function getErrorReportRecord(contextKey: string): ErrorReportRecord | undefined {
  initPersistenceIfNeeded();
  return reportRecords.get(contextKey);
}

/**
 * Marks an error context as in-flight for background auto telemetry.
 * Returns true if this is the first attempt, or false if already in-flight/reported or too old.
 */
export function markAutoReportInFlight(contextKey: string, occurredAt?: string): boolean {
  if (!isErrorFreshForAutoReport(occurredAt)) {
    return false;
  }
  const record = ensureRecord(contextKey);
  if (record.autoStatus === 'in_flight' || record.autoStatus === 'reported') {
    return false;
  }
  record.autoStatus = 'in_flight';
  return true;
}

/**
 * Marks an error context as successfully auto-reported.
 */
export function markAutoReportSuccess(contextKey: string, now: number = Date.now()): void {
  const record = ensureRecord(contextKey);
  record.autoStatus = 'reported';
  record.autoReportedAt = now;
  persistRecords();
}

/**
 * Marks an error context's auto-report attempt as failed so manual submission can retry.
 */
export function markAutoReportFailed(contextKey: string): void {
  const record = ensureRecord(contextKey);
  record.autoStatus = 'failed';
}

/**
 * Checks whether user submission should be deduplicated (skipped without actual network dispatch).
 *
 * Rules:
 * 1. If base report was already auto-reported (or in-flight) and user provided NO description
 *    and NO screenshots, deduplicate! (Base logs already monitored).
 * 2. If user already submitted a supplementary report and submits identical content & screenshots again, deduplicate!
 * 3. If auto-report failed or user provides new content/screenshots, do not deduplicate.
 */
export function shouldDeduplicateUserSubmission(
  contextKey: string,
  draft: UserSubmissionDraftLike
): boolean {
  initPersistenceIfNeeded();
  const record = reportRecords.get(contextKey);
  if (!record || record.autoStatus === 'idle' || record.autoStatus === 'failed') {
    return false;
  }

  const { count: screenshotsCount, fingerprint: screenshotsFingerprint } =
    buildScreenshotsFingerprint(draft.screenshots, draft.screenshotsCount ?? 0);

  const trimmedDescription = draft.description.trim();
  const hasNoSupplement = trimmedDescription === '' && screenshotsCount === 0;

  // Case 1: Base error already reported/in-flight, user provided no supplement
  if (hasNoSupplement) {
    return true;
  }

  // Case 2: User re-submits exact same content and screenshots
  if (
    record.lastSubmittedDescription !== undefined &&
    trimmedDescription === record.lastSubmittedDescription &&
    record.lastSubmittedScreenshotsFingerprint !== undefined &&
    screenshotsFingerprint === record.lastSubmittedScreenshotsFingerprint
  ) {
    return true;
  }

  // Fallback for cases where only screenshot count is compared
  if (
    record.lastSubmittedDescription !== undefined &&
    trimmedDescription === record.lastSubmittedDescription &&
    record.lastSubmittedScreenshotsFingerprint === undefined &&
    screenshotsCount === (record.lastSubmittedScreenshotsCount ?? 0)
  ) {
    return true;
  }

  return false;
}

/**
 * Records that a user manual submission was successful.
 */
export function recordUserSubmissionSuccess(
  contextKey: string,
  draft: UserSubmissionDraftLike
): void {
  const record = ensureRecord(contextKey);
  const { count: screenshotsCount, fingerprint: screenshotsFingerprint } =
    buildScreenshotsFingerprint(draft.screenshots, draft.screenshotsCount ?? 0);

  record.autoStatus = 'reported';
  record.lastSubmittedDescription = draft.description.trim();
  record.lastSubmittedScreenshotsCount = screenshotsCount;
  record.lastSubmittedScreenshotsFingerprint = screenshotsFingerprint;
  record.supplementCount += 1;
  persistRecords();
}

/**
 * Resets tracking cache for unit tests.
 */
export function resetErrorReportTrackingForTests(): void {
  reportRecords.clear();
  persistenceInitialized = false;
  try {
    if (typeof window !== 'undefined' && window.localStorage) {
      window.localStorage.removeItem(STORAGE_KEY);
    }
  } catch {
    // Ignore storage failure in tests
  }
}
