/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export const IN_FLIGHT = new Set(['loading', 'queued', 'running', 'cancelling']);
export const TIER_ORDER = ['smoke', 'capability', 'imported', 'advanced', 'sandbox'] as const;
export type EvalTier = (typeof TIER_ORDER)[number];

export function formatRate(value: number): string {
  return `${(value * 100).toFixed(1)}%`;
}

export function formatAvg(value: number): string {
  return Number.isFinite(value) ? value.toFixed(1) : '0';
}

export function formatElapsed(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return '—';
  if (ms >= 60_000) {
    const minutes = Math.floor(ms / 60_000);
    const seconds = Math.round((ms % 60_000) / 1000);
    return `${minutes}m ${seconds}s`;
  }
  if (ms >= 1000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.round(ms)}ms`;
}

export function shortId(id: string, length = 8): string {
  return id.replace(/[^a-zA-Z0-9]/g, '').slice(0, length) || id.slice(0, length);
}

export function statusColor(status: string): string {
  switch (status) {
    case 'completed':
      return 'green';
    case 'failed':
      return 'red';
    case 'cancelled':
    case 'cancelling':
      return 'gray';
    default:
      return 'arcoblue';
  }
}

export function isImportedSuiteId(id: string | undefined | null): boolean {
  return Boolean(id?.startsWith('imported-'));
}

export function isOfficeValSuiteId(id: string | undefined | null): boolean {
  return id === 'omegause_officeval' || id === 'officeval';
}

export function isTrialsLockedSuite(id: string | undefined | null): boolean {
  return isImportedSuiteId(id) || isOfficeValSuiteId(id);
}

export type EvalTaskProfile = 'office' | 'coding';

export function normalizeTaskProfile(value: string | undefined | null): EvalTaskProfile {
  return value === 'coding' ? 'coding' : 'office';
}

export function preferredSuiteId(profile: EvalTaskProfile): string {
  return profile === 'coding' ? 'coding_local' : 'office_core';
}

export function caseRowKey(caseId: string, trial?: number): string {
  return `${caseId}#${trial ?? 1}`;
}

export function eventKindColor(kind: string, isError?: boolean | null): string {
  if (isError || kind === 'error') return 'red';
  switch (kind) {
    case 'tool_call':
      return 'arcoblue';
    case 'tool_result':
      return 'green';
    case 'thinking':
      return 'orangered';
    default:
      return 'gray';
  }
}
