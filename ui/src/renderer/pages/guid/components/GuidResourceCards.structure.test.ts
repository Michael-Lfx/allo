/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Guid homepage single-screen layout', () => {
  test('keeps only the conversation stage and does not mount scroll discovery content', () => {
    const source = readSource(new URL('../GuidPage.tsx', import.meta.url));

    const inputIndex = source.indexOf('<GuidInputCard');
    const primaryStageIndex = source.indexOf('className={styles.guidPrimaryStage}');
    const editorHostIndex = source.indexOf('<GuidAssistantEditorHost', inputIndex);

    expect(primaryStageIndex).toBeGreaterThan(-1);
    expect(inputIndex).toBeGreaterThan(-1);
    expect(editorHostIndex).toBeGreaterThan(inputIndex);
    expect(source.includes('guidDiscoveryArea')).toBe(false);
    expect(source.includes('GuidCompanionPosterPreview')).toBe(false);
    expect(source.includes('GuidResourceCards')).toBe(false);
    expect(source.includes('onFillPrompt')).toBe(false);
  });

  test('allows vertical scroll on guidContainer so short viewports can reach editor-host content', () => {
    const css = readSource(new URL('../index.module.css', import.meta.url));
    const block =
      css.match(/\.guidContainer\s*\{[^}]*\}/)?.[0] ?? '';

    expect(block.includes('overflow-y: auto')).toBe(true);
    expect(block.includes('overflow: hidden')).toBe(false);
  });

  test('contains docs, promo video, and contact feedback cards without recent prompt data access', () => {
    const source = readSource(new URL('./GuidResourceCards.tsx', import.meta.url));

    expect(source.includes('https://www.nomifun.com/docs')).toBe(true);
    expect(source.includes('https://www.bilibili.com/video/BV1kwKZ6UE5X/')).toBe(true);
    expect(source.includes('https://youtu.be/AsEToBDFR9s')).toBe(true);
    expect(source.includes('https://youtu.be/gEDo5H0H0Pg')).toBe(false);
    expect(source.includes('https://www.nomifun.com/contact')).toBe(true);
    expect(source.includes('https://github.com/nomifun/nomifun-tauri/issues')).toBe(false);
    expect(source.includes('RECENT_PROMPT_LIMIT')).toBe(false);
    expect(source.includes('getConversationMessages')).toBe(false);
    expect(source.includes('useConversationHistoryContext')).toBe(false);
    expect(source.includes('useSWR')).toBe(false);
    expect(source.includes('onFillPrompt')).toBe(false);
  });
});
