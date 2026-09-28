/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import {
  StreamingHighlightCache,
  ensureShikiLanguage,
  tokenizeCode,
  tokenizeCodeStreaming,
} from './shikiHighlighter';

const SAMPLE = `function greet(name: TMessage) {
  const id = message.msg_id;
  foo.bar(id);
  return id;
}`;

const tokenColor = (
  lines: { content: string; style: Record<string, string> }[][],
  word: string,
  key = '--shiki-light'
) => {
  for (const line of lines) {
    for (const token of line) {
      if (token.content === word) return token.style[key];
    }
  }
  return undefined;
};

describe('Shiki highlighter runtime', () => {
  test('colors function calls, types, and parameters that highlight.js 10 leaves plain', async () => {
    await ensureShikiLanguage('typescript');
    const lines = tokenizeCode(SAMPLE, 'typescript');
    expect(lines).not.toBeNull();
    const tokens = lines!.flat();
    const byContent = Object.fromEntries(tokens.map((token) => [token.content, token.style]));

    expect(byContent.function?.['--shiki-light']).toBe('#CF222E');
    expect(byContent.greet?.['--shiki-light']).toBe('#8250DF');
    expect(byContent.TMessage?.['--shiki-light']).toBe('#953800');
    expect(byContent.name?.['--shiki-light']).toBe('#953800');
    expect(byContent.bar?.['--shiki-light']).toBe('#8250DF');
    expect(byContent.const?.['--shiki-light']).toBe('#CF222E');
    expect(byContent.function?.['--shiki-dark']).toBe('#FF7B72');
    expect(tokens.some((token) => token.style['--shiki-light'] && token.style['--shiki-dark'])).toBe(true);
  });

  test('re-highlights only the last streaming line after a newline', async () => {
    await ensureShikiLanguage('typescript');
    const cache = new StreamingHighlightCache();
    const first = tokenizeCodeStreaming('const x = 1', 'typescript', true, cache);
    expect(first).not.toBeNull();
    expect(first!.length).toBe(1);
    expect(first![0].some((token) => token.content === 'const' && token.style['--shiki-light'] === '#CF222E')).toBe(
      true
    );

    const grown = tokenizeCodeStreaming('const x = 1\nfoo.bar()', 'typescript', true, cache);
    expect(grown).not.toBeNull();
    expect(grown!.length).toBe(2);
    expect(grown![0].some((token) => token.content === 'const')).toBe(true);
    expect(grown![1].some((token) => token.content === 'bar' && token.style['--shiki-light'] === '#8250DF')).toBe(
      true
    );
  });

  test('reuses the streaming snapshot when the finished content matches', async () => {
    await ensureShikiLanguage('typescript');
    const cache = new StreamingHighlightCache();
    const streamed = tokenizeCodeStreaming(SAMPLE, 'typescript', true, cache);
    const reused = cache.snapshotIfMatches(SAMPLE, 'typescript');
    const finished = tokenizeCodeStreaming(SAMPLE, 'typescript', false, cache);
    expect(reused).not.toBeNull();
    expect(reused).toEqual(streamed);
    expect(finished).toEqual(streamed);
    expect(cache.snapshotIfMatches(`${SAMPLE}\n`, 'typescript')).toBeNull();
  });

  test('highlights JSX tags with tsx and embedded JS with html', async () => {
    const jsxSample = 'const el = <Button className="ok">{name}</Button>;';
    await ensureShikiLanguage('tsx');
    await ensureShikiLanguage('typescript');
    const tsxLines = tokenizeCode(jsxSample, 'tsx');
    const tsLines = tokenizeCode(jsxSample, 'typescript');
    expect(tsxLines).not.toBeNull();
    expect(tsLines).not.toBeNull();
    expect(tokenColor(tsxLines!, 'Button')).toBe('#116329');
    expect(tokenColor(tsLines!, 'Button')).not.toBe('#116329');

    const htmlSample = '<style>body{color:red}</style><script>const x = 1</script>';
    await ensureShikiLanguage('html');
    await ensureShikiLanguage('xml');
    const htmlLines = tokenizeCode(htmlSample, 'html');
    const xmlLines = tokenizeCode(htmlSample, 'xml');
    expect(htmlLines).not.toBeNull();
    expect(xmlLines).not.toBeNull();
    expect(tokenColor(htmlLines!, 'const')).toBe('#CF222E');
    expect(tokenColor(xmlLines!, 'const')).not.toBe('#CF222E');
  });

  test('renders unknown fences as plain text immediately', () => {
    const lines = tokenizeCode('TypeError: boom', 'text');
    expect(lines).not.toBeNull();
    expect(lines![0][0]?.content).toBe('TypeError: boom');
    expect(lines![0][0]?.style['--shiki-light']).toBeUndefined();
  });

  test('prefetches common grammars on idle and does not swallow regex errors', () => {
    const source = readFileSync(new URL('./shikiHighlighter.ts', import.meta.url), 'utf8');
    expect(source.includes('requestIdleCallback')).toBe(true);
    expect(source.includes("'tsx'")).toBe(true);
    expect(source.includes('createJavaScriptRegexEngine()')).toBe(true);
    expect(source.includes('forgiving')).toBe(false);
    expect(source.includes("codeToTokens('//\\n'")).toBe(true);
  });
});
