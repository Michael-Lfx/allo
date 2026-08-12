import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./SettingsPageWrapper.tsx', import.meta.url), 'utf8');
const styles = readFileSync(new URL('./settings.css', import.meta.url), 'utf8');

describe('SettingsPageWrapper layout contract', () => {
  test('keeps a single vertical page owner and exposes form and hub widths', () => {
    expect(source).toContain("layout?: 'form' | 'hub'");
    expect(source).toContain("layout: layoutMode = 'form'");
    expect(source).toContain("'app-page-shell settings-page-wrapper w-full min-h-full box-border overflow-y-auto'");
    expect(source).toContain("data-settings-page-content");
    expect(source).toContain("settings-page-content--hub");
    expect(source).toContain("settings-page-content--form");
  });

  test('localizes the narrow-window navigation label', () => {
    expect(source).toContain("const { t } = useTranslation()");
    expect(source).toContain("aria-label={t('settings.title')}");
    expect(source).not.toContain("aria-label='Settings'");
  });

  test('moves the settings tab indicator without animating layout width', () => {
    const inkStyles = styles.slice(
      styles.indexOf('.flowy-settings-tabs .arco-tabs-header-ink {'),
      styles.indexOf('.flowy-settings-tabs .arco-tabs-header-title:focus-visible')
    );
    expect(inkStyles).toContain('transition: transform');
    expect(inkStyles).not.toContain('width var(--flowy-motion-state)');
  });
});
