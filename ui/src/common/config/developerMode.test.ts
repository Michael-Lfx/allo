/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import {
  DEVELOPER_GATED_TAB_IDS,
  filterDeveloperGatedTabs,
  isDeveloperGatedTabId,
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

  test('verifyDeveloperModePassword accepts the configured unlock phrase', () => {
    expect(verifyDeveloperModePassword('whosyourdaddy')).toBe(true);
    expect(verifyDeveloperModePassword('  whosyourdaddy  ')).toBe(true);
    expect(verifyDeveloperModePassword('wrong')).toBe(false);
  });
});
