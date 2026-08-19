/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export function shortId(id: string | null | undefined, head = 8): string {
  if (!id) return '-';
  if (id.length <= head + 4) return id;
  return `${id.slice(0, head)}…`;
}

export function formatJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

/** Format elapsed milliseconds as `2.5s` / `1m5s`, not raw `65730ms`. */
export function formatDurationMs(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms) || ms < 0) return '';
  if (ms < 1000) {
    const seconds = ms / 1000;
    if (seconds <= 0) return '0s';
    return `${seconds.toFixed(1).replace(/\.0$/, '')}s`;
  }
  const totalSeconds = ms / 1000;
  if (totalSeconds < 60) {
    const rounded = totalSeconds < 10 ? totalSeconds.toFixed(1) : String(Math.round(totalSeconds));
    return `${rounded.replace(/\.0$/, '')}s`;
  }
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = Math.round(totalSeconds % 60);
  return `${minutes}m${seconds}s`;
}

export function formatClock(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms)) return '';
  try {
    return new Date(ms).toLocaleTimeString(undefined, {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
  } catch {
    return '';
  }
}

export function turnToolCount(turn: { model_calls: Array<{ tools: unknown[] }> }): number {
  return turn.model_calls.reduce((sum, call) => sum + call.tools.length, 0);
}

/** Round numbers follow `started_at_ms` ascending, independent of display order. */
export function assignTurnRounds(
  entries: Array<{ root_turn_id: string; started_at_ms?: number | null }>
): Map<string, number> {
  const indexed = entries.map((turn, index) => ({ turn, index }));
  indexed.sort((a, b) => {
    const aTime = a.turn.started_at_ms;
    const bTime = b.turn.started_at_ms;
    if (aTime != null && bTime != null && aTime !== bTime) return aTime - bTime;
    if (aTime != null && bTime == null) return -1;
    if (aTime == null && bTime != null) return 1;
    return a.index - b.index;
  });
  const rounds = new Map<string, number>();
  indexed.forEach((item, index) => {
    rounds.set(item.turn.root_turn_id, index + 1);
  });
  return rounds;
}
