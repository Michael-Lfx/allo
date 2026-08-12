import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./SettingsPagePrimitives.tsx', import.meta.url), 'utf8');

describe('settings page spacing contract', () => {
  test('keeps legacy helpers while providing a shared list and row contract', () => {
    expect(source).toContain('mb-12px ml-12px pl-12px md:ml-20px md:pl-20px');
    expect(source).toContain("classNames('flex pt-16px', className)");
    expect(source).toContain("'flowy-settings-list'");
    expect(source).toContain("'flowy-settings-row'");
    expect(source).toContain("'flowy-settings-status'");
    expect(source).toContain("role={tone === 'error' ? 'alert' : 'status'}");
  });

  test('provides the full detail-page contract without creating a third component system', () => {
    expect(source).toContain('SettingsPageHeader');
    expect(source).toContain('SettingsPermissionRow');
    expect(source).toContain('SettingsEmptyState');
    expect(source).toContain('SettingsActionBar');
    expect(source).toContain("'compact' | 'field' | 'actions' | 'compound'");
    expect(source).toContain('SettingsControlGroup');
    expect(source).toContain("'success' | 'warning' | 'error' | 'restart-required'");
    expect(source).toContain("'flowy-settings-row--interactive'");
    expect(source).toContain('export const SettingsSection = SettingsGroup');
  });

  test('keeps control layout semantic so translated actions do not inherit a field width', () => {
    expect(source).toContain("controlLayout = 'field'");
    expect(source).toContain('flowy-settings-row__control--${controlLayout}');
    expect(source).toContain('flowy-settings-control-group');
  });
});
