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

/** Vite-like env slice used to distinguish `bun run dev` from packaged builds. */
export type DeveloperModeBuildEnv = {
  DEV?: boolean;
  PROD?: boolean;
};

/** Taps on Settings → About title required to reveal the developer-mode switch. */
export const DEVELOPER_MODE_REVEAL_TAP_COUNT = 5;

export function nextDeveloperModeRevealTap(currentTaps: number): {
  taps: number;
  justRevealed: boolean;
} {
  const taps = currentTaps + 1;
  return {
    taps,
    justRevealed: taps === DEVELOPER_MODE_REVEAL_TAP_COUNT,
  };
}

/**
 * Official installers hide the unlock UI until About-title taps reveal it.
 * Local `bun run dev` / `dev:web` keep the unlock UI.
 */
export function isDeveloperModeUiEnabled(
  env: DeveloperModeBuildEnv = import.meta.env,
  revealed = false
): boolean {
  return env.DEV === true || revealed === true;
}

/**
 * Stored `system.developerMode` only unlocks gated surfaces in local dev builds
 * or after the About-title reveal in production packages.
 */
export function isDeveloperModeActive(
  pref: boolean | undefined,
  env: DeveloperModeBuildEnv = import.meta.env,
  revealed = false
): boolean {
  return isDeveloperModeUiEnabled(env, revealed) && pref === true;
}
