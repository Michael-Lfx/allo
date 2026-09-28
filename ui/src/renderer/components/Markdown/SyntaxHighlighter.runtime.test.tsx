/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { beautifulUiHighlightStyle } from '@renderer/components/beautifulUi/codeBlock/codeBlockHighlight';
import SyntaxHighlighter from './SyntaxHighlighter';
import { resolveSyntaxLanguage } from './syntaxLanguage';

describe('Markdown syntax highlighter runtime', () => {
  test('highlights a common alias through the Highlight.js grammar registry', () => {
    const html = renderToStaticMarkup(
      <SyntaxHighlighter language={resolveSyntaxLanguage('js')} PreTag='div'>
        {'const answer = 42;\n'}
      </SyntaxHighlighter>
    );

    expect(html.includes('const')).toBe(true);
    expect(html.includes('answer')).toBe(true);
    expect(html.includes('hljs-keyword') || html.includes('const')).toBe(true);
  });

  test('applies Beautiful UI GitHub token CSS variables to highlighted spans', () => {
    const html = renderToStaticMarkup(
      <SyntaxHighlighter
        language={resolveSyntaxLanguage('ts')}
        style={beautifulUiHighlightStyle}
        PreTag='div'
      >
        {'const answer = 42;\nfunction greet(name: string) {\n  return name;\n}\n'}
      </SyntaxHighlighter>
    );

    expect(html.includes('var(--code-token-keyword')).toBe(true);
    expect(html.includes('var(--code-token-constant')).toBe(true);
    expect(html.includes('var(--code-token-function')).toBe(true);
    expect(html.includes('var(--code-token-type')).toBe(true);
    expect(html.includes('const')).toBe(true);
    expect(html.includes('greet')).toBe(true);
  });

  test('emits hljs class names when inline token styles are disabled', () => {
    const html = renderToStaticMarkup(
      <SyntaxHighlighter
        language={resolveSyntaxLanguage('ts')}
        style={beautifulUiHighlightStyle}
        useInlineStyles={false}
        PreTag='div'
      >
        {'const answer = 42;\n'}
      </SyntaxHighlighter>
    );

    expect(html.includes('hljs-keyword')).toBe(true);
    expect(html.includes('const')).toBe(true);
  });

  test('renders copied error output as plain text', () => {
    const html = renderToStaticMarkup(
      <SyntaxHighlighter language={resolveSyntaxLanguage('log')} PreTag='div'>
        {'TypeError: emitter.startScope is not a function\n    at highlightAuto (...)'}
      </SyntaxHighlighter>
    );

    expect(html.includes('TypeError: emitter.startScope is not a function')).toBe(true);
    expect(html.includes('highlightAuto')).toBe(true);
  });
});
