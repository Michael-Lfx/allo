/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Guid homepage single-screen layout', () => {
  test('keeps the conversation stage and mounts three in-app demo path cards', () => {
    const source = readSource(new URL('../GuidPage.tsx', import.meta.url));

    const inputIndex = source.indexOf('<GuidInputCard');
    const primaryStageIndex = source.indexOf('className={styles.guidPrimaryStage}');
    const editorHostIndex = source.indexOf('<GuidPresetEditorHost', inputIndex);

    expect(primaryStageIndex).toBeGreaterThan(-1);
    expect(inputIndex).toBeGreaterThan(-1);
    expect(editorHostIndex).toBeGreaterThan(inputIndex);
    expect(source.includes('GuidResourceCards')).toBe(true);
    expect(source.includes('guidDiscoveryArea')).toBe(false);
    expect(source.includes('GuidCompanionPosterPreview')).toBe(false);
  });

  test('allows vertical scroll on guidContainer so short viewports can reach editor-host content', () => {
    const css = readSource(new URL('../index.module.css', import.meta.url));
    const block = css.match(/\.guidContainer\s*\{[^}]*\}/)?.[0] ?? '';

    expect(block.includes('overflow-y: auto')).toBe(true);
    expect(block.includes('overflow: hidden')).toBe(false);
  });

  test('GuidResourceCards exposes three in-app CTAs without marketing outbound links', () => {
    const source = readSource(new URL('./GuidResourceCards.tsx', import.meta.url));

    expect(source.includes('guid-path-local-agent')).toBe(true);
    expect(source.includes('guid-path-companion')).toBe(true);
    expect(source.includes('guid-path-open-caps')).toBe(true);
    expect(source.includes("navigate('/nomi')")).toBe(true);
    expect(source.includes("navigate('/open-capabilities')")).toBe(true);
    expect(source.includes('https://www.nomifun.com/docs')).toBe(false);
    expect(source.includes('openExternalUrl')).toBe(false);
    expect(source.includes('RECENT_PROMPT_LIMIT')).toBe(false);
  });
});
