import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./CodeBlock.tsx', import.meta.url), 'utf8');

describe('Markdown fenced CodeBlock Beautiful UI wrap', () => {
  test('wraps fenced code in the Beautiful UI shell and keeps the highlighter', () => {
    expect(source.includes("from '@renderer/components/beautifulUi/codeBlock/CodeBlock'")).toBe(true);
    expect(source.includes('<BeautifulUiCodeBlock')).toBe(true);
    expect(source.includes('ShikiCodeFence')).toBe(true);
    expect(source.includes('filename={filename}')).toBe(true);
    expect(source.includes('highlighted=')).toBe(true);
    expect(source.includes("overflowX: 'clip'")).toBe(true);
    expect(source.includes("overflowX: 'visible'")).toBe(false);
    expect(source.includes("width: 'fit-content'")).toBe(false);
  });

  test('pins highlighter font to --code-font at 12.5px and defers streaming highlight', () => {
    expect(source.includes('CODE_LINE_HEIGHT_PX')).toBe(true);
    expect(source.includes('useDeferredValue')).toBe(true);
    expect(source.includes('React.memo')).toBe(true);
    expect(source.includes('const CODE_LINE_HEIGHT = CODE_LINE_HEIGHT_PX')).toBe(true);
    expect(source.includes('11.5px')).toBe(false);
  });

  test('keeps the token tree stable across expand and markdown re-renders', () => {
    expect(source.includes('useMemo(() => formatCode(children)')).toBe(true);
    expect(source.includes('const highlightContent = isStreaming ? deferredContent : formattedContent')).toBe(true);
    expect(source.includes('function areCodeBlockPropsEqual')).toBe(true);
    expect(source.includes('const CodeFenceHighlight = React.memo')).toBe(true);
    expect(source.includes('React.memo(CodeBlock, areCodeBlockPropsEqual)')).toBe(true);
    expect(source.includes('isStreaming={isStreaming}')).toBe(true);
  });

  test('lazy-loads Mermaid instead of statically importing the diagram runtime', () => {
    expect(source.includes("React.lazy(() => import('./MermaidBlock'))")).toBe(true);
    expect(source.includes("import MermaidBlock from './MermaidBlock'")).toBe(false);
  });

  test('styles inline code as a compact mono chip', () => {
    const inlineStart = source.indexOf("if (!String(children).includes('\\n'))");
    const inlineBlock = source.slice(inlineStart, source.indexOf('const totalLines', inlineStart));
    expect(inlineStart).toBeGreaterThan(-1);
    expect(inlineBlock.includes('<code')).toBe(true);
    expect(inlineBlock.includes('BeautifulUiCodeBlock')).toBe(false);
    expect(inlineBlock.includes('INLINE_CODE_STYLE')).toBe(true);
    expect(source.includes("background: 'var(--code-bg)'")).toBe(true);
    expect(source.includes("padding: '2px 6px'")).toBe(true);
    expect(source.includes("fontWeight: 'bold'")).toBe(false);
  });

  test('clips collapsed preview to three line rows, not body padding', () => {
    expect(source.includes('const PREVIEW_LINES = 3')).toBe(true);
    expect(source.includes('CODE_PADDING_VERTICAL')).toBe(false);
    expect(source.includes('PREVIEW_LINES * CODE_LINE_HEIGHT')).toBe(true);
    expect(source.includes('+ CODE_PADDING_VERTICAL')).toBe(false);
  });

  test('uses Lucide chevrons for expand and collapse', () => {
    expect(source.includes("from 'lucide-react'")).toBe(true);
    expect(source.includes('ChevronDown')).toBe(true);
    expect(source.includes('ChevronUp')).toBe(true);
    expect(source.includes('@icon-park/react')).toBe(false);
  });
});
