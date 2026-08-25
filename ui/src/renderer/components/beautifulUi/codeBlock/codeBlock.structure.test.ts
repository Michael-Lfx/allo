import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./CodeBlock.tsx', import.meta.url), 'utf8');
const cssSource = readFileSync(new URL('./codeBlock.module.css', import.meta.url), 'utf8');

const cssRuleFor = (selector: string): string => {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = cssSource.match(new RegExp(`${escaped}\\s*\\{[\\s\\S]*?\\}`));
  return match?.[0] ?? '';
};

describe('CodeBlock', () => {
  test('covers Beautiful UI fenced code chrome without inventing message types', () => {
    expect(source.includes("export type CodeBlockProps = {")).toBe(true);
    expect(source.includes('language?: string')).toBe(true);
    expect(source.includes('filename?: string')).toBe(true);
    expect(source.includes('displayNameForCodeLanguage')).toBe(true);
    expect(source.includes('styles.filename')).toBe(true);
    expect(source.includes('styles.language')).toBe(true);
    expect(source.includes('styles.lineNo')).toBe(true);
    expect(source.includes('children: string')).toBe(true);
    expect(source.includes('streaming?: boolean')).toBe(true);
    expect(source.includes("data-testid='beautiful-ui-code-block'")).toBe(true);
    expect(source.includes("data-testid='beautiful-ui-code-block-language'")).toBe(true);
    expect(source.includes("data-testid='beautiful-ui-code-block-copy'")).toBe(true);
    expect(source.includes("TMessageType")).toBe(false);
    expect(cssSource.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
    expect(cssSource.includes('white-space: pre-wrap')).toBe(true);
    expect(cssRuleFor('.body').includes('overflow: visible')).toBe(true);
    expect(cssRuleFor('.body').includes('overflow-x: auto')).toBe(false);
    expect(cssRuleFor('pre.body').includes('overflow-x: clip')).toBe(true);
    expect(cssRuleFor('pre.body').includes('overflow-x: auto')).toBe(false);
    expect(cssRuleFor('.lineText').includes('white-space: pre-wrap')).toBe(true);
    expect(cssRuleFor('.lineText').includes('overflow-wrap: anywhere')).toBe(true);
  });

  test('uses Beautiful UI Lucide copy glyphs instead of IconPark', () => {
    expect(source.includes("from 'lucide-react'")).toBe(true);
    expect(source.includes('<Copy ')).toBe(true);
    expect(source.includes('<Check ')).toBe(true);
    expect(source.includes("t('common.copy')")).toBe(true);
    expect(source.includes("t('common.copySuccess')")).toBe(true);
    expect(source.includes('@icon-park/react')).toBe(false);
  });

  test('keeps the streaming code caret out of wrap width', () => {
    const caretRule = cssSource.slice(
      cssSource.indexOf('.streaming .line:last-child .lineText::after'),
      cssSource.indexOf('@keyframes code-block-caret')
    );
    expect(caretRule.includes('margin-right: -5px')).toBe(true);
    expect(caretRule.includes('width: 3px')).toBe(true);
    expect(caretRule.includes('margin-left: 2px')).toBe(true);
  });

  test('uses an accent streaming caret without a primary glow', () => {
    expect(cssSource.includes('linear-gradient')).toBe(false);
    expect(cssSource.includes('0 0 8px')).toBe(false);
    expect(cssSource.includes('background: var(--color-primary-6')).toBe(true);
  });
});
