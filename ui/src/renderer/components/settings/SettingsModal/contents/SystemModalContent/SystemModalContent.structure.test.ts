import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');

describe('system settings visual contract', () => {
  test('uses five stable sections and keeps notification rows mounted', () => {
    for (const key of [
      'settings.sectionGeneral',
      'settings.sectionNotifications',
      'settings.sectionStorage',
      'settings.sectionDeveloper',
      'settings.sectionDanger',
    ]) {
      expect(source).toContain(key);
    }
    expect(source).not.toContain('<Collapse');
    expect(source).toContain('disabled={!notificationEnabled}');
    expect(source).toContain('settings.disabledByNotifications');
    expect(source).toContain('isDeveloperModeUiEnabled()');
    expect(source).toContain('{isDeveloperModeUiEnabled() && (');
  });

  test('restores local state and reports a localized failure when auto-save fails', () => {
    expect(source).toContain('configService.setLocal(key, previous)');
    expect(source).toContain("t('settings.preferenceSaveFailed')");
    expect(source).toContain('setSendKey(previous)');
  });

  test('does not sync the Flowy model catalog when the compact vision picker mounts', () => {
    expect(source).toContain('<ImageAnalysisModelContent compact autoRefreshCatalog={false} />');
  });

  test('retains relocation, restart, retry, cancel, backup, and factory reset flows', () => {
    for (const marker of [
      'workDirRelocation.retry.invoke',
      'workDirRelocation.cancel.invoke',
      'application.restart.invoke',
      'workDirRelocationBackup',
      '<FactoryResetModal',
    ]) {
      expect(source).toContain(marker);
    }
  });
});
