/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export function shortId(id: string | null | undefined, head = 8): string {
  if (!id) return '—';
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
