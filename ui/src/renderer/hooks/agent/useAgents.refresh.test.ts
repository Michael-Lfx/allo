/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = () => readFileSync(new URL('./useAgents.ts', import.meta.url), 'utf8');

describe('useAgents detection refresh wiring', () => {
  test('keeps the throttled refresh available without calling it from the hook', () => {
    const text = source();

    expect(text.includes('AGENT_AUTO_REFRESH_MIN_INTERVAL_MS')).toBe(true);
    expect(text.includes('createAgentRefreshScheduler')).toBe(true);
    expect(text.includes('export const refreshDetectedAgentsIfStale')).toBe(true);
    expect(text.includes('useEffect')).toBe(false);
  });

  test('explicit refresh delegates to the snapshot replacement flow', () => {
    const text = source();

    expect(text.includes('refreshAgentAvailability')).toBe(true);
    expect(text.includes('refreshSnapshot: () => ipcBridge.acpConversation.refreshCustomAgents.invoke()')).toBe(true);
    expect(text.includes('replaceCachedSnapshot: (agents) => mutate(DETECTED_AGENTS_SWR_KEY, agents, { revalidate: false })')).toBe(true);
  });

  test('does not revalidate a prefetched snapshot during the first hook mount', () => {
    const text = source();

    expect(text.includes('DETECTED_AGENTS_SWR_OPTIONS')).toBe(true);
  });

  test('keeps the production refresh dependencies wired to the shared cache', () => {
    const text = source();

    expect(text.includes('refreshSnapshot: () => ipcBridge.acpConversation.refreshCustomAgents.invoke()')).toBe(true);
    expect(text.includes('revalidate: false')).toBe(true);
  });
});
