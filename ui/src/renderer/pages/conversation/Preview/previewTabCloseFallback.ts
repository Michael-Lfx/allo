/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * Pick which tab should become active after closing the current one.
 *
 * Prefer the previously focused tab when it still exists; otherwise fall back
 * to the last remaining tab (legacy preview behavior).
 */
export const resolveActiveTabAfterClose = (
  remainingTabs: ReadonlyArray<{ id: string }>,
  previousActiveTabId: string | null | undefined
): string | null => {
  if (remainingTabs.length === 0) return null;

  if (
    previousActiveTabId &&
    remainingTabs.some((tab) => tab.id === previousActiveTabId)
  ) {
    return previousActiveTabId;
  }

  return remainingTabs[remainingTabs.length - 1]?.id ?? null;
};
