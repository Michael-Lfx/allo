import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./CodeBlock.tsx', import.meta.url), 'utf8');

describe('Markdown fenced CodeBlock Beautiful UI wrap', () => {
  test('wraps fenced code in the Beautiful UI shell and keeps the highlighter', () => {
    expect(source.includes("from '@renderer/components/beautifulUi/codeBlock/CodeBlock'")).toBe(true);
    expect(source.includes('<BeautifulUiCodeBlock')).toBe(true);
    expect(source.includes('beautifulUiHighlightStyle')).toBe(true);
    expect(source.includes('showLineNumbers')).toBe(true);
    expect(source.includes('filename={filename}')).toBe(true);
    expect(source.includes('highlighted=')).toBe(true);
    expect(source.includes('SyntaxHighlighter')).toBe(true);
    expect(source.includes("overflowX: 'clip'")).toBe(true);
    expect(source.includes('wrapLongLines')).toBe(true);
    expect(source.includes("whiteSpace: 'pre-wrap'")).toBe(true);
    expect(source.includes("overflowX: 'visible'")).toBe(false);
    expect(source.includes("width: 'fit-content'")).toBe(false);
  });

  test('lazy-loads Mermaid instead of statically importing the diagram runtime', () => {
    expect(source.includes("React.lazy(() => import('./MermaidBlock'))")).toBe(true);
    expect(source.includes("import MermaidBlock from './MermaidBlock'")).toBe(false);
  });

  test('does not wrap inline code in the Beautiful UI shell', () => {
    const inlineStart = source.indexOf("if (!String(children).includes('\\n'))");
    const inlineBlock = source.slice(inlineStart, source.indexOf('const isDiff', inlineStart));
    expect(inlineStart).toBeGreaterThan(-1);
    expect(inlineBlock.includes('<code')).toBe(true);
    expect(inlineBlock.includes('BeautifulUiCodeBlock')).toBe(false);
  });

  test('uses Lucide chevrons for expand and collapse', () => {
    expect(source.includes("from 'lucide-react'")).toBe(true);
    expect(source.includes('ChevronDown')).toBe(true);
    expect(source.includes('ChevronUp')).toBe(true);
    expect(source.includes('@icon-park/react')).toBe(false);
  });
});
