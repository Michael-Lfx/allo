/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('CodeBlock streaming behavior', () => {
  test('forces expanded layout and tail scroll while streaming', () => {
    const source = readSource(new URL('./CodeBlock.tsx', import.meta.url));
    const shikiSource = readSource(new URL('./ShikiCodeFence.tsx', import.meta.url));
    const hljsSource = readSource(new URL('./HljsCodeFence.tsx', import.meta.url));

    expect(source.includes('isStreaming?: boolean')).toBe(true);
    expect(source.includes('const isEffectivelyExpanded = isStreaming || expanded')).toBe(true);
    expect(source.includes("overflowX: 'clip'")).toBe(true);
    expect(source.includes("overflowY: isStreaming ? 'auto' : 'clip'")).toBe(true);
    expect(source.includes("overflowX: 'visible'")).toBe(false);
    expect(source.includes('ResizeObserver')).toBe(false);
    expect(source.includes("overflowY: isStreaming ? 'auto' : 'hidden'")).toBe(false);
    expect(source.includes('canCollapse && !isStreaming')).toBe(true);
    expect(source.includes('node.scrollTop = node.scrollHeight')).toBe(true);
    expect(source.includes('streaming={isStreaming}')).toBe(false);
    expect(source.includes('streaming={false}')).toBe(true);
    expect(source.includes('useDeferredValue')).toBe(true);
    expect(source.includes('ShikiCodeFence')).toBe(true);
    expect(source.includes('isStreaming={isStreaming}')).toBe(true);
    expect(shikiSource.includes('tokenizeCodeStreaming')).toBe(true);
    expect(shikiSource.includes('HljsCodeFence')).toBe(true);
    expect(hljsSource.includes('wrapLongLines')).toBe(true);
    expect(hljsSource.includes("whiteSpace: 'pre-wrap'")).toBe(true);
    expect(hljsSource.includes("overflow: 'visible'")).toBe(true);
    expect(hljsSource.includes("width: '100%'")).toBe(true);
    expect(hljsSource.includes("marginRight: '10px'")).toBe(false);
  });

  test('conversation streaming activates Markdown only after a code fence begins', () => {
    const source = readSource(
      new URL('../../pages/conversation/Messages/components/MessageText.tsx', import.meta.url)
    );

    expect(source.includes('splitStreamingMarkdown')).toBe(true);
    expect(source.includes("from '@renderer/components/beautifulUi/codeBlock/CodeBlock'")).toBe(true);
    expect(source.includes("streamingParts.tailKind === 'code'")).toBe(true);
  });

  test('uses Shadow DOM-owned toolbar classes instead of document utility classes', () => {
    const source = readSource(new URL('./CodeBlock.tsx', import.meta.url));

    expect(source.includes("className='markdown-code-block'")).toBe(true);
    expect(source.includes("className='markdown-code-toolbar'")).toBe(true);
    expect(source.includes("className='markdown-code-action'")).toBe(true);
    expect(source.includes('group-hover')).toBe(false);
  });

  test('uses Shadow DOM theme classes for code chrome and footer interaction', () => {
    const source = readSource(new URL('./CodeBlock.tsx', import.meta.url));
    const shadowSource = readSource(new URL('./ShadowView.tsx', import.meta.url));

    expect(source.includes('<BeautifulUiCodeBlock')).toBe(true);
    expect(source.includes("className='markdown-code-footer'")).toBe(true);
    expect(source.includes("data-testid='markdown-code-footer'")).toBe(true);
    expect(source.includes("data-collapsible={canCollapse ? 'true' : 'false'}")).toBe(true);
    expect(source.includes("data-streaming={isStreaming ? 'true' : 'false'}")).toBe(true);
    expect(source.includes("className='markdown-code-footer-label'")).toBe(true);
    expect(source.includes('from \'lucide-react\'')).toBe(true);
    expect(source.includes('rgba(255,255,255,0.55)')).toBe(false);
    expect(shadowSource.includes('.markdown-code-footer:hover')).toBe(true);
    expect(shadowSource.includes('.markdown-code-footer:active')).toBe(true);
    expect(shadowSource.includes('.markdown-code-footer:focus-visible')).toBe(true);
    expect(shadowSource.includes("'--code-font'")).toBe(true);
    expect(shadowSource.includes("'--code-token-keyword'")).toBe(true);
    expect(shadowSource.includes("'--code-bg'")).toBe(true);
    expect(shadowSource.includes("shadowRoot.host.setAttribute('data-theme'")).toBe(true);
    expect(shadowSource.includes('.markdown-code-content .shiki span')).toBe(false);
    expect(shadowSource.includes('var(--shiki-dark)')).toBe(false);
    const shikiSource = readSource(new URL('./ShikiCodeFence.tsx', import.meta.url));
    expect(shikiSource.includes('useMemo')).toBe(true);
    expect(shikiSource.includes('snapshotIfMatches')).toBe(true);
  });
});
