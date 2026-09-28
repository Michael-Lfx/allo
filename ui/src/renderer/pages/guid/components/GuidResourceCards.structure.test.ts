
import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Guid homepage single-screen layout', () => {
  test('keeps the conversation stage without task-intent starter cards', () => {
    const source = readSource(new URL('../GuidPage.tsx', import.meta.url));

    const inputIndex = source.indexOf('<GuidInputCard');
    const primaryStageIndex = source.indexOf('className={styles.guidPrimaryStage}');
    const editorHostIndex = source.indexOf('<GuidPresetEditorHost', inputIndex);

    expect(primaryStageIndex).toBeGreaterThan(-1);
    expect(inputIndex).toBeGreaterThan(primaryStageIndex);
    expect(editorHostIndex).toBeGreaterThan(inputIndex);
    expect(source.includes('GuidResourceCards')).toBe(false);
    expect(source.includes('<TaskProfileSelector')).toBe(true);
    const profileIndex = source.indexOf('<TaskProfileSelector');
    expect(profileIndex).toBeGreaterThan(primaryStageIndex);
    expect(profileIndex).toBeLessThan(inputIndex);
    expect(source.includes('GuidReadinessStrip')).toBe(false);
    expect(source.includes("data-testid='guid-run-settings-toggle'")).toBe(false);
    expect(source.includes("data-testid='guid-run-settings-panel'")).toBe(false);
    expect(source.includes('guidDiscoveryArea')).toBe(false);
    expect(source.includes('GuidCompanionPosterPreview')).toBe(false);
    expect(source.includes('QuickActionButtons')).toBe(false);
  });

  test('allows vertical scroll on guidContainer so short viewports can reach editor-host content', () => {
    const css = readSource(new URL('../index.module.css', import.meta.url));
    const block = css.match(/\.guidContainer\s*\{[^}]*\}/)?.[0] ?? '';

    expect(block.includes('overflow-y: auto')).toBe(true);
    expect(block.includes('overflow: hidden')).toBe(false);
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
