/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export function formatElapsed(ms: number | null | undefined): string {
  if (ms == null || Number.isNaN(ms)) return '—';
  if (ms < 1000) return `${Math.round(ms)}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(2)}s`;
  const minutes = Math.floor(ms / 60_000);
  const seconds = ((ms % 60_000) / 1000).toFixed(1);
  return `${minutes}m ${seconds}s`;
}

export function formatClock(ms: number | null | undefined): string {
  if (ms == null || Number.isNaN(ms) || ms <= 0) return '—';
  try {
    return new Date(ms).toLocaleString(undefined, {
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hour12: false,
    });
  } catch {
    return String(ms);
  }
}

export function formatTokenCount(n: number | null | undefined): string {
  if (n == null || Number.isNaN(n)) return '0';
  return n.toLocaleString();
}

export function shortId(id: string | null | undefined, head = 8): string {
  if (!id) return '—';
  if (id.length <= head + 4) return id;
  return `${id.slice(0, head)}…`;
}

export function contextOccupancyPercent(
  contextTokens: number | null | undefined,
  contextWindow: number | null | undefined
): number | null {
  if (!contextWindow || contextWindow <= 0 || contextTokens == null) return null;
  return Math.min(100, Math.round((contextTokens / contextWindow) * 1000) / 10);
}

export function outcomeLabel(
  success: boolean | null | undefined,
  stopReason?: string | null
): 'ok' | 'fail' | 'cancelled' | 'unknown' {
  if (success === false) return 'fail';
  if (stopReason === 'cancelled') return 'cancelled';
  if (success === true) return 'ok';
  return 'unknown';
}

export function formatJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

export function formatBytes(n: number | null | undefined): string {
  if (n == null || Number.isNaN(n) || n < 0) return '—';
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(2)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}
