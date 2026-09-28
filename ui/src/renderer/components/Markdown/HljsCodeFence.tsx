/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useMemo } from 'react';
import { beautifulUiHighlightStyle } from '@renderer/components/beautifulUi/codeBlock/codeBlockHighlight';
import { CODE_FONT_SIZE_PX, CODE_GUTTER_FONT_SIZE_PX, CODE_LINE_HEIGHT_PX } from './codeFenceLayout';
import { getDiffLineStyle } from './markdownUtils';
import SyntaxHighlightBoundary from './SyntaxHighlightBoundary';
import SyntaxHighlighter from './SyntaxHighlighter';

const EMPTY_DIFF_LINES: string[] = [];
const MemoSyntaxHighlighter = React.memo(SyntaxHighlighter);

const highlighterCustomStyle: React.CSSProperties = {
  margin: 0,
  padding: 0,
  borderRadius: 0,
  border: 'none',
  background: 'transparent',
  color: 'var(--color-text-2, #4e5969)',
  overflow: 'visible',
  maxWidth: '100%',
  minWidth: 0,
  width: '100%',
  fontFamily: 'var(--code-font)',
  fontSize: `${CODE_FONT_SIZE_PX}px`,
  lineHeight: `${CODE_LINE_HEIGHT_PX}px`,
  whiteSpace: 'pre-wrap',
};

const highlighterCodeStyle: React.CSSProperties = {
  color: 'inherit',
  background: 'transparent',
  display: 'block',
  maxWidth: '100%',
  minWidth: 0,
  overflow: 'visible',
  overflowWrap: 'anywhere',
  whiteSpace: 'pre-wrap',
  wordBreak: 'break-word',
  fontFamily: 'var(--code-font)',
  fontSize: `${CODE_FONT_SIZE_PX}px`,
  lineHeight: `${CODE_LINE_HEIGHT_PX}px`,
};

const highlighterLineNumberStyle: React.CSSProperties = {
  minWidth: '20px',
  paddingRight: '10px',
  marginRight: 0,
  color: 'color-mix(in srgb, var(--color-text-3, #86909c) 75%, transparent)',
  fontSize: `${CODE_GUTTER_FONT_SIZE_PX}px`,
  lineHeight: `${CODE_LINE_HEIGHT_PX}px`,
  textAlign: 'right',
  userSelect: 'none',
};

export type HljsCodeFenceProps = {
  content: string;
  language: string;
  isDiff?: boolean;
  isDark?: boolean;
  diffLines?: string[];
  showLineNumbers?: boolean;
};

const HljsCodeFence = React.memo(function HljsCodeFence({
  content,
  language,
  isDiff = false,
  isDark = false,
  diffLines = EMPTY_DIFF_LINES,
  showLineNumbers = true,
}: HljsCodeFenceProps) {
  const lineProps = useCallback(
    (lineNumber: number) => ({
      style: {
        display: 'block' as const,
        minWidth: 0,
        ...(isDiff ? getDiffLineStyle(diffLines[lineNumber - 1] || '', isDark) : {}),
      },
    }),
    [diffLines, isDark, isDiff]
  );
  const codeTagProps = useMemo(() => ({ style: { ...highlighterCodeStyle } }), []);
  const fallback = (
    <div style={highlighterCustomStyle} data-syntax-highlight-fallback>
      <code style={highlighterCodeStyle}>{content}</code>
    </div>
  );

  return (
    <SyntaxHighlightBoundary fallback={fallback} resetKey={`${language}\u0000${content}`}>
      <MemoSyntaxHighlighter
        children={content}
        language={language}
        style={beautifulUiHighlightStyle}
        useInlineStyles={false}
        showLineNumbers={showLineNumbers}
        PreTag='div'
        wrapLongLines
        wrapLines
        lineNumberStyle={highlighterLineNumberStyle}
        lineProps={lineProps}
        customStyle={highlighterCustomStyle}
        codeTagProps={codeTagProps}
      />
    </SyntaxHighlightBoundary>
  );
});

export default HljsCodeFence;
