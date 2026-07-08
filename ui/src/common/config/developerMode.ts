/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** Settings tabs that stay hidden until developer mode is unlocked. */
export const DEVELOPER_GATED_TAB_IDS = ['cloud-login'] as const;

export type DeveloperGatedTabId = (typeof DEVELOPER_GATED_TAB_IDS)[number];

export function isDeveloperGatedTabId(tabId: string): tabId is DeveloperGatedTabId {
  return (DEVELOPER_GATED_TAB_IDS as readonly string[]).includes(tabId);
}

export function filterDeveloperGatedTabs<T extends string>(
  tabIds: readonly T[],
  developerModeEnabled: boolean
): T[] {
  if (developerModeEnabled) {
    return [...tabIds];
  }
  return tabIds.filter((id) => !isDeveloperGatedTabId(id));
}

/**
 * Temporary UX gate for advanced settings — not a security boundary.
 * Replace with a server-side check when a real unlock flow is available.
 */
const DEVELOPER_MODE_UNLOCK_PHRASE = 'whosyourdaddy';

export function verifyDeveloperModePassword(input: string): boolean {
  return input.trim() === DEVELOPER_MODE_UNLOCK_PHRASE;
}
