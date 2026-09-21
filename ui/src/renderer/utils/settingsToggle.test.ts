import { describe, expect, test } from 'bun:test';

import { resolveSettingsTogglePath } from './settingsToggle';

describe('settings toggle path', () => {
  test('enters system settings from an app route', () => {
    expect(resolveSettingsTogglePath('/guid')).toEqual({ enter: true, path: '/settings/system' });
    expect(resolveSettingsTogglePath('/conversation/abc')).toEqual({ enter: true, path: '/settings/system' });
  });

  test('leaves settings to the last non-settings path', () => {
    expect(resolveSettingsTogglePath('/settings/system', '/conversation/abc')).toEqual({
      enter: false,
      path: '/conversation/abc',
    });
    expect(resolveSettingsTogglePath('/settings/system', '/settings/about')).toEqual({
      enter: false,
      path: '/guid',
    });
  });
});
