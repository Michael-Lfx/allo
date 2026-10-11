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

  test('bsk-0-01 smoke test script is properly authored and archived', () => {
    const bskUrl = new URL('../../../../../docs/architecture/voice-agent/execution/bsk-0-01-settings-shell.md', import.meta.url);
    expect(existsSync(bskUrl)).toBe(true);
    const bskSource = readFileSync(bskUrl, 'utf8');
    expect(bskSource).toContain('bsk-0-01');
    expect(bskSource).toContain('SettingsRow');
    expect(bskSource).toContain('scaleX');
  });

  test('T1-09: connects to /api/voice/models/progress SSE and triggers /install POST', () => {
    const source = readFileSync(componentUrl, 'utf8');

    expect(source).toContain('/api/voice/models/progress');
    expect(source).toContain('/api/voice/models/install');
  });

  test('T1-09: supports all visual states (Probing, Downloading, Verifying, Ready, Failed) with inline retry', () => {
    const source = readFileSync(componentUrl, 'utf8');

    // States handling
    expect(source).toContain('ProbingSource');
    expect(source).toContain('Downloading');
    expect(source).toContain('VerifyingSha256');
    expect(source).toContain('Ready');
    expect(source).toContain('Failed');

    // Inline retry action
    expect(source).toContain('voice.actions.retry');
  });

  test('T1-09: applies 200ms throttle to SSE progress events', () => {
    const source = readFileSync(componentUrl, 'utf8');

    // Throttling contract per ui-motion.zh.md §3
    expect(source).toMatch(/throttle|200/);
  });

  test('T1-09: bsk-1-01 verification script is properly authored and archived', () => {
    const bsk1Url = new URL('../../../../../docs/architecture/voice-agent/execution/bsk-1-01-progress-motion.md', import.meta.url);
    expect(existsSync(bsk1Url)).toBe(true);
    const bsk1Source = readFileSync(bsk1Url, 'utf8');
    expect(bsk1Source).toContain('bsk-1-01');
    expect(bsk1Source).toContain('scaleX');
    expect(bsk1Source).toContain('tabular-nums');
  });
});
