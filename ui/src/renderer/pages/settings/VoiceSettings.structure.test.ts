/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { existsSync, readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const componentUrl = new URL('./VoiceSettings.tsx', import.meta.url);
const navUrl = new URL('./components/settingsNavigation.ts', import.meta.url);
const routerUrl = new URL('../../components/layout/Router.tsx', import.meta.url);

describe('VoiceSettings structure & ui-motion contract (T0-16 / bsk-0-01)', () => {
  test('VoiceSettings.tsx exists and can be read', () => {
    expect(existsSync(componentUrl)).toBe(true);
  });

  test('conforms to Page Header -> Section -> List -> Row hierarchy and prohibits Card/Modal', () => {
    const source = readFileSync(componentUrl, 'utf8');

    // Hierarchy primitives
    expect(source).toContain('SettingsPageWrapper');
    expect(source).toContain("layout='form'");
    expect(source).toContain('SettingsPageHeader');
    expect(source).toMatch(/SettingsSection|SettingsGroup/);
    expect(source).toContain('SettingsList');
    expect(source).toContain('SettingsRow');

    // Prohibited anti-patterns per ui-motion.zh.md §3 and §5
    expect(source).not.toContain('<Card');
    expect(source).not.toContain('arco-card');
    expect(source).not.toContain('<Modal');
    expect(source).not.toContain('arco-modal');
  });

  test('renders 2px hairline progress track driven by scaleX without layout animation', () => {
    const source = readFileSync(componentUrl, 'utf8');

    // 2px hairline track
    expect(source).toMatch(/h-2px|2px/);
    // Driven by scaleX transform
    expect(source).toContain('scaleX');
    // No animating layout properties like width or height
    expect(source).not.toMatch(/transition:\s*width/);
    expect(source).not.toMatch(/animate-width/);
  });

  test('uses semantic i18n keys for all user-facing texts', () => {
    const source = readFileSync(componentUrl, 'utf8');

    expect(source).toContain('useTranslation');
    expect(source).toContain('voice.actions.downloadAndEnable');
    expect(source).toMatch(/voice\.phases\.notInstalled|voice\.description/);
  });

  test('fetches real status from /api/voice/models/status', () => {
    const source = readFileSync(componentUrl, 'utf8');

    expect(source).toContain('/api/voice/models/status');
  });

  test('settings navigation includes voice in capabilities group with voice icon', () => {
    const navSource = readFileSync(navUrl, 'utf8');

    expect(navSource).toContain("id: 'voice'");
    expect(navSource).toContain("path: 'voice'");
    expect(navSource).toContain("groupId: 'capabilities'");
    expect(navSource).toContain("icon: 'voice'");
    expect(navSource).toContain("labelKey: 'voice.settings.nav'");
  });

  test('Router.tsx registers /settings/voice route with route fallback', () => {
    const routerSource = readFileSync(routerUrl, 'utf8');

    expect(routerSource).toContain("path='/settings/voice'");
    expect(routerSource).toContain('VoiceSettings');
  });
});
