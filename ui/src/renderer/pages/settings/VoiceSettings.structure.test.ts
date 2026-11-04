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

  test('settings rows stack by container width, never by a viewport media query', () => {
    const settingsCss = readFileSync(
      new URL('./components/settings.css', import.meta.url),
      'utf8'
    );
    // High-DPI desktop windows can be narrower than the 960px content column in
    // CSS pixels; a `@media (max-width: 767px)` row override stacked every row
    // on those windows even though the card had room. Row structure is owned by
    // the container queries (760/640) only.
    expect(settingsCss).not.toMatch(
      /@media\s*\(max-width:\s*767px\)[\s\S]*?\.flowy-settings-row\s*\{[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\)/
    );
    expect(settingsCss).toContain('@container (max-width: 760px)');
    expect(settingsCss).toContain('@container (max-width: 640px)');
  });

  test('T2-07: Uninstall asks for confirmation in the shared dialog, then calls DELETE', () => {
    const source = readFileSync(componentUrl, 'utf8');

    // Destructive uninstall goes through the shared Modal.confirm pattern; the
    // dropdown closes on the first click, so the old inline two-step menu (and
    // its 3s revert timer) must stay gone.
    expect(source).toContain('Modal.confirm');
    expect(source).toContain("t('voice.actions.uninstall')");
    expect(source).toContain("t('voice.actions.uninstallConfirmTitle')");
    expect(source).toContain("t('voice.actions.uninstallConfirmContent')");
    expect(source).not.toContain('confirmRelease');
    expect(source).not.toContain('isConfirmingRelease');
    // Confirming still triggers DELETE /api/voice/models with DELETE auth headers
    expect(source).toContain('/api/voice/models');
    expect(source).toMatch(/DELETE/);
  });

  test('T2-08: prefers-reduced-motion disables transitions and scales instantaneously', () => {
    const source = readFileSync(componentUrl, 'utf8');

    // Evaluates prefers-reduced-motion media query
    expect(source).toContain('prefers-reduced-motion');
    // Disables motion (transition none / 0ms / transform none)
    expect(source).toMatch(/reducedMotion|prefersReducedMotion/);
  });
});


test('install/delete render the backend receipt instead of a bare 2xx success', () => {
  const source = readFileSync(componentUrl, 'utf8');
  expect(source).toContain('parseVoiceModelOperation');
  expect(source).toContain('await response.text()');
  expect(source).toContain('outcome.receipt');
  expect(source).toContain('connectProgressStream');
  // The old unconditional 2xx rewrite must stay gone.
  expect(source).not.toMatch(/response\.ok\s*\|\|\s*response\.status === 202/);
});
test('full-call readiness exposes an explicit check and repair reason', () => {
  const source = readFileSync(componentUrl, 'utf8');
  expect(source).toContain('runVoiceSelfCheck');
  expect(source).toContain("voice.capabilities.title");
  expect(source).toContain("voice.capabilities.textOnly");
  expect(source).toContain("handleSelfCheck");
  expect(source).toContain("useTextReply");
  expect(source).toContain("requestDictationIntent('/')");
  expect(source).not.toContain("activateExistingDictation");
  expect(source).not.toMatch(/setTimeout\([\s\S]{0,120}activateExistingDictation/);
  expect(source).not.toContain("onClick={handleUseTextReply}>\\n");
});
test('first-call failure exposes an explicit prepare action', () => {
  const source = readFileSync(componentUrl, 'utf8');
  const guid = readFileSync(new URL('../guid/GuidPage.tsx', import.meta.url), 'utf8');
  expect(guid).toContain('prepareVoiceSession');
  expect(guid).not.toContain('voice.entry.prepareHint');
});

test('voice settings keep model identifiers inside advanced diagnostics and remove wake overclaims',()=>{
 const source=readFileSync(componentUrl,'utf8');
 expect(source.indexOf('<VoiceDialogueStatusSection')).toBeGreaterThan(source.indexOf("voice.diagnostics.advancedTitle"));
 expect(source).not.toContain('voice.dashboard.tipContent');
 expect(source).not.toContain('voice.dashboard.activeKeywordsList');
});

test('verifying state shows the progress ratio once under a phase title and a real hint', () => {
  const source = readFileSync(componentUrl, 'utf8');
  // Title carries the phase name; the count is rendered once in the control slot.
  expect(source).toContain("t('voice.status.verifyingSha256')");
  expect(source).toContain("t('voice.phases.verifyingSha256Desc')");
  // The count-bearing phase string must never be reused as label *and* description.
  expect(source).not.toMatch(/rowLabel\s*=\s*t\('voice\.phases\.verifyingSha256'/);
  expect(source).not.toMatch(/rowDescription\s*=\s*t\('voice\.phases\.verifyingSha256'/);
});

test('readiness chain wraps without orphan connectors or horizontal overflow', () => {
  const source = readFileSync(componentUrl, 'utf8');
  // Connector and pill share one flexible unit, so a wrapped row cannot leave a
  // dangling arrow at the end of the previous line.
  expect(source).toMatch(/<span key=\{name\} className='inline-flex min-w-0 items-center gap-2'>/);
  expect(source).toMatch(/aria-hidden='true'[^>]*>→<\/span>/);
  // Pills may shrink and wrap their label instead of pushing past the card edge.
  expect(source).toContain('min-w-0 break-words');
  expect(source).toContain('whitespace-nowrap');
  // Page-local fixed-width action wrappers stranded stacked controls mid-card
  // instead of following the shared SettingsRow container-query behaviour.
  expect(source).not.toContain('w-240px shrink-0');
});

