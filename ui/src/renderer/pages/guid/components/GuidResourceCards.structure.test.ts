
import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Guid homepage single-screen layout', () => {
  test('keeps the conversation stage without task-intent starter cards', () => {
    const source = readSource(new URL('../GuidPage.tsx', import.meta.url));
    const actionRow = readSource(new URL('./GuidActionRow.tsx', import.meta.url));

    const inputIndex = source.indexOf('<GuidInputCard');
    const primaryStageIndex = source.indexOf('className={styles.guidPrimaryStage}');
    const editorHostIndex = source.indexOf('<GuidPresetEditorHost', inputIndex);

    expect(primaryStageIndex).toBeGreaterThan(-1);
    expect(inputIndex).toBeGreaterThan(primaryStageIndex);
    expect(editorHostIndex).toBeGreaterThan(inputIndex);
    expect(source.includes('GuidResourceCards')).toBe(false);
    expect(source.includes('<GuidActionRow')).toBe(true);
    expect(actionRow.includes('<TaskProfileSelector')).toBe(true);
    const actionRowNodeIndex = source.indexOf('const actionRowNode');
    expect(actionRowNodeIndex).toBeGreaterThan(-1);
    expect(actionRowNodeIndex).toBeLessThan(inputIndex);
    expect(source.includes('actionRow={actionRowNode}')).toBe(true);
    expect(source.includes('GuidReadinessStrip')).toBe(false);
    expect(source.includes("data-testid='guid-run-settings-toggle'")).toBe(false);
    expect(source.includes("data-testid='guid-run-settings-panel'")).toBe(false);
    expect(source.includes('guidDiscoveryArea')).toBe(false);
    expect(source.includes('GuidCompanionPosterPreview')).toBe(false);
    expect(source.includes('QuickActionButtons')).toBe(false);
  });

  test('places the sidebar wordmark above the homepage cards without the retired video shortcut', () => {
    const source = readSource(new URL('../GuidPage.tsx', import.meta.url));
    const primaryStageIndex = source.indexOf('className={styles.guidPrimaryStage}');
    const brandIndex = source.indexOf('<GuidHomeBrand');
    const featureIndex = source.indexOf('<FeatureCards');

    expect(brandIndex).toBeGreaterThan(primaryStageIndex);
    expect(featureIndex).toBeGreaterThan(brandIndex);
    expect(source.includes('<GuidVideoShortcut')).toBe(false);
  });

  test('keeps vertical scrolling inside guidScrollArea on short viewports', () => {
    const css = readSource(new URL('../index.module.css', import.meta.url));
    const container = css.match(/\.guidContainer\s*\{[^}]*\}/)?.[0] ?? '';
    const scrollArea = css.match(/\.guidScrollArea\s*\{[^}]*\}/)?.[0] ?? '';

    expect(container.includes('overflow: hidden')).toBe(true);
    expect(/overflow-y:\s*(?:auto|scroll)/.test(scrollArea)).toBe(true);
  });

  test('does not retain the retired companion-poster experiment', () => {
    const css = readSource(new URL('../index.module.css', import.meta.url));
    const zh = readSource(new URL('../../../services/i18n/locales/zh-CN/conversation.json', import.meta.url));
    const en = readSource(new URL('../../../services/i18n/locales/en-US/conversation.json', import.meta.url));

    expect(css.includes('guidCompanionPoster')).toBe(false);
    expect(zh.includes('"companionPoster"')).toBe(false);
    expect(en.includes('"companionPoster"')).toBe(false);
  });
});
