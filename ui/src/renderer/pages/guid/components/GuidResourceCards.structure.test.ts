

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Guid homepage single-screen layout', () => {
  test('keeps the conversation stage and mounts task-intent cards', () => {
    const source = readSource(new URL('../GuidPage.tsx', import.meta.url));

    const inputIndex = source.indexOf('<GuidInputCard');
    const intentIndex = source.indexOf('<GuidResourceCards');
    const primaryStageIndex = source.indexOf('className={styles.guidPrimaryStage}');
    const editorHostIndex = source.indexOf('<GuidPresetEditorHost', inputIndex);

    expect(primaryStageIndex).toBeGreaterThan(-1);
    expect(intentIndex).toBeGreaterThan(primaryStageIndex);
    expect(inputIndex).toBeGreaterThan(-1);
    expect(intentIndex).toBeLessThan(inputIndex);
    expect(editorHostIndex).toBeGreaterThan(inputIndex);
    expect(source.includes('GuidResourceCards')).toBe(true);
    expect(source.includes('GuidReadinessStrip')).toBe(true);
    expect(source.includes("data-testid='guid-run-settings-panel'")).toBe(true);
    expect(source.includes('guidDiscoveryArea')).toBe(false);
    expect(source.includes('GuidCompanionPosterPreview')).toBe(false);
  });

  test('allows vertical scroll on guidContainer so short viewports can reach editor-host content', () => {
    const css = readSource(new URL('../index.module.css', import.meta.url));
    const block = css.match(/\.guidContainer\s*\{[^}]*\}/)?.[0] ?? '';

    expect(block.includes('overflow-y: auto')).toBe(true);
    expect(block.includes('overflow: hidden')).toBe(false);
  });

  test('GuidResourceCards exposes task intents that fill the composer without outbound marketing links', () => {
    const source = readSource(new URL('./GuidResourceCards.tsx', import.meta.url));
    const readiness = readSource(new URL('../readiness/guidReadiness.ts', import.meta.url));

    expect(readiness.includes("id: 'fix-code'")).toBe(true);
    expect(readiness.includes("id: 'summarize'")).toBe(true);
    expect(readiness.includes("id: 'automate'")).toBe(true);
    expect(source.includes('guid-intent-')).toBe(true);
    expect(source.includes('onSetInput')).toBe(true);
    expect(source.includes('intentsForWorkspace')).toBe(true);
    expect(source.includes("navigate('/nomi')")).toBe(false);
    expect(source.includes("navigate('/open-capabilities')")).toBe(false);
    expect(source.includes('https://www.nomifun.com/docs')).toBe(false);
    expect(source.includes('openExternalUrl')).toBe(false);
    expect(source.includes('RECENT_PROMPT_LIMIT')).toBe(false);
  });
});
