/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import {
  DEVELOPER_GATED_TAB_IDS,
  DEVELOPER_MODE_REVEAL_TAP_COUNT,
  filterDeveloperGatedTabs,
  isDeveloperGatedTabId,
  isDeveloperModeActive,
  isDeveloperModeUiEnabled,
  nextDeveloperModeRevealTap,
  verifyDeveloperModePassword,
} from './developerMode';

describe('developerMode', () => {
  test('cloud-login is the only developer-gated tab', () => {
    expect(DEVELOPER_GATED_TAB_IDS).toEqual(['cloud-login']);
    expect(isDeveloperGatedTabId('cloud-login')).toBe(true);
    expect(isDeveloperGatedTabId('system')).toBe(false);
  });

  test('filterDeveloperGatedTabs hides cloud-login until developer mode is enabled', () => {
    const tabs = ['system', 'media', 'cloud-login', 'about'] as const;
    expect(filterDeveloperGatedTabs(tabs, false)).toEqual(['system', 'media', 'about']);
    expect(filterDeveloperGatedTabs(tabs, true)).toEqual(['system', 'media', 'cloud-login', 'about']);
  });

  test('shows the cloud-account module in production packages after developer mode is enabled', () => {
    const tabs = ['system', 'media', 'cloud-login', 'about'] as const;
    expect(isDeveloperModeActive(true, { DEV: false, PROD: true }, true)).toBe(true);
    expect(filterDeveloperGatedTabs(tabs, true)).toEqual(['system', 'media', 'cloud-login', 'about']);
    expect(filterDeveloperGatedTabs(tabs, false)).toEqual(['system', 'media', 'about']);
  });

  test('verifyDeveloperModePassword accepts the configured unlock phrase', () => {
    expect(verifyDeveloperModePassword('whosyourdaddy')).toBe(true);
    expect(verifyDeveloperModePassword('  whosyourdaddy  ')).toBe(true);
    expect(verifyDeveloperModePassword('wrong')).toBe(false);
  });

  test('hides the developer-mode setting in production packages', () => {
    expect(isDeveloperModeUiEnabled({ DEV: false, PROD: true })).toBe(false);
    expect(isDeveloperModeUiEnabled({ DEV: true, PROD: false })).toBe(true);
  });

  test('ignores a stored developerMode pref in production packages', () => {
    expect(isDeveloperModeActive(true, { DEV: false, PROD: true })).toBe(false);
    expect(isDeveloperModeActive(true, { DEV: true, PROD: false })).toBe(true);
    expect(isDeveloperModeActive(false, { DEV: true, PROD: false })).toBe(false);
    expect(isDeveloperModeActive(undefined, { DEV: true, PROD: false })).toBe(false);
  });

  test('five About title taps reveal developer-mode UI in production packages', () => {
    expect(DEVELOPER_MODE_REVEAL_TAP_COUNT).toBe(5);
    expect(nextDeveloperModeRevealTap(0)).toEqual({ taps: 1, justRevealed: false });
    expect(nextDeveloperModeRevealTap(3)).toEqual({ taps: 4, justRevealed: false });
    expect(nextDeveloperModeRevealTap(4)).toEqual({ taps: 5, justRevealed: true });
    expect(nextDeveloperModeRevealTap(5)).toEqual({ taps: 6, justRevealed: false });
  });

  test('a revealed pref shows developer-mode UI in production packages', () => {
    expect(isDeveloperModeUiEnabled({ DEV: false, PROD: true }, true)).toBe(true);
    expect(isDeveloperModeUiEnabled({ DEV: false, PROD: true }, false)).toBe(false);
    expect(isDeveloperModeActive(true, { DEV: false, PROD: true }, true)).toBe(true);
    expect(isDeveloperModeActive(true, { DEV: false, PROD: true }, false)).toBe(false);
    expect(isDeveloperModeActive(false, { DEV: false, PROD: true }, true)).toBe(false);
  });
});
