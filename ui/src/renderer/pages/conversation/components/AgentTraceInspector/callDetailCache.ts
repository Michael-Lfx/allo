/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ProjectedModelCall } from './useAgentTraces';

export const MAX_CALL_DETAIL_CACHE = 2;

export class CallDetailLru {
  private readonly entries = new Map<string, ProjectedModelCall>();

  get(key: string): ProjectedModelCall | undefined {
    const value = this.entries.get(key);
    if (value === undefined) return undefined;
    this.entries.delete(key);
    this.entries.set(key, value);
    return value;
  }

  set(key: string, value: ProjectedModelCall): void {
    if (this.entries.has(key)) this.entries.delete(key);
    this.entries.set(key, value);
    while (this.entries.size > MAX_CALL_DETAIL_CACHE) {
      const oldest = this.entries.keys().next().value;
      if (oldest === undefined) break;
      this.entries.delete(oldest);
    }
  }

  clear(): void {
    this.entries.clear();
  }

  get size(): number {
    return this.entries.size;
  }
}

export function callCacheKey(
  conversationId: string,
  rootTurnId: string,
  modelCallId: string
): string {
  return `${conversationId}:${rootTurnId}:${modelCallId}`;
}
