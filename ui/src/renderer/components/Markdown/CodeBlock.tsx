

import { ChevronDown, ChevronUp } from 'lucide-react';
import katex from 'katex';
import React, { useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import BeautifulUiCodeBlock from '@renderer/components/beautifulUi/codeBlock/CodeBlock';
import { beautifulUiHighlightStyle } from '@renderer/components/beautifulUi/codeBlock/codeBlockHighlight';
import { filenameFromFenceNode } from '@renderer/components/beautifulUi/codeBlock/codeBlockLanguage';
import MermaidBlock from './MermaidBlock';
import { formatCode, getDiffLineStyle } from './markdownUtils';
import SyntaxHighlighter from './SyntaxHighlighter';

const PREVIEW_LINES = 3;
const CODE_LINE_HEIGHT = 20;
const CODE_PADDING_VERTICAL = 20;
const COLLAPSED_HEIGHT = PREVIEW_LINES * CODE_LINE_HEIGHT + CODE_PADDING_VERTICAL;

type CodeBlockProps = {
  children: string;
  className?: string;
  node?: unknown;
  hiddenCodeCopyButton?: boolean;
  codeStyle?: React.CSSProperties;
  isStreaming?: boolean;
  [key: string]: unknown;
};

function CodeBlock(props: CodeBlockProps) {
  const { children, className, node: _node, hiddenCodeCopyButton, codeStyle: _c, isStreaming = false, ...rest } = props;
  const { t } = useTranslation();
  const blockId = useId();
  const [expanded, setExpanded] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const [currentTheme, setCurrentTheme] = useState<'light' | 'dark'>(
    () => (document.documentElement.getAttribute('data-theme') as 'light' | 'dark') || 'light'
  );

  React.useEffect(() => {
    const update = () => {
      setCurrentTheme((document.documentElement.getAttribute('data-theme') as 'light' | 'dark') || 'light');
    };
    const observer = new MutationObserver(update);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    return () => observer.disconnect();
  }, []);

  const toggleExpanded = () => {
    const willCollapse = expanded;
    setExpanded((v) => !v);
    if (willCollapse && containerRef.current) {
      requestAnimationFrame(() => {
        containerRef.current?.scrollIntoView({ block: 'nearest', behavior: 'auto' });
      });
    }
  };

  const match = /language-(\w+)/.exec(className || '');
  const language = match?.[1] || 'text';
  const filename = filenameFromFenceNode(props.node);

  // KaTeX math blocks
  if (language === 'latex' || language === 'math' || language === 'tex') {
    const latexSource = String(children).replace(/\n$/, '');
    const isFullDocument = /\\(documentclass|begin\{document\}|usepackage)\b/.test(latexSource);
    if (!isFullDocument) {
      try {
        const html = katex.renderToString(latexSource, { displayMode: true, throwOnError: false });
        return <div className='katex-display' dangerouslySetInnerHTML={{ __html: html }} />;
      } catch {
        // fall through
      }
    }
  }

  if (language === 'mermaid') {
    return <MermaidBlock code={formatCode(children)} style={props.codeStyle} />;
  }

  // Inline code (single line)
  if (!String(children).includes('\n')) {
    return (
      <code {...rest} className={className} style={{ fontWeight: 'bold' }}>
        {children}
      </code>
    );
  }

  const isDiff = language === 'diff';
  const formattedContent = formatCode(children);
  const totalLines = formattedContent.split('\n').length;
  const canCollapse = totalLines > PREVIEW_LINES;
  const isEffectivelyExpanded = isStreaming || expanded;
  const diffLines = isDiff ? formattedContent.split('\n') : [];
  const isDark = currentTheme === 'dark';

  const codeContentId = `${blockId}-content`;
  const footerId = `${blockId}-footer`;

  return (
    <div
      ref={containerRef}
      style={{ width: '100%', minWidth: 0, maxWidth: '100%', ...props.codeStyle }}
      className='markdown-code-block'
    >
      <BeautifulUiCodeBlock
        language={language}
        filename={filename}
        streaming={isStreaming}
        hiddenCopyButton={hiddenCodeCopyButton}
        toolbar={
          canCollapse && !isStreaming ? (
            <div className='markdown-code-toolbar'>
              <button
                type='button'
                title={expanded ? t('common.collapse') : t('common.expand')}
                aria-label={expanded ? t('common.collapse') : t('common.expand')}
                aria-expanded={expanded}
                aria-controls={codeContentId}
                className='markdown-code-action'
                onClick={toggleExpanded}
              >
                {expanded ? (
                  <ChevronUp size={14} strokeWidth={1.75} aria-hidden />
                ) : (
                  <ChevronDown size={14} strokeWidth={1.75} aria-hidden />
                )}
              </button>
            </div>
          ) : undefined
        }
        highlighted={
          <div
            ref={(node) => {
              contentRef.current = node;
              if (node && isStreaming) {
                node.scrollTop = node.scrollHeight;
              }
            }}
            id={codeContentId}
            className='markdown-code-content'
            style={{
              maxHeight: canCollapse && !isEffectivelyExpanded ? `${COLLAPSED_HEIGHT}px` : 'none',
              overflowX: 'clip',
              overflowY: isStreaming ? 'auto' : 'clip',
            }}
          >
            <SyntaxHighlighter
              children={formattedContent}
              language={language}
              style={beautifulUiHighlightStyle}
              showLineNumbers
              PreTag='div'
              wrapLongLines
              wrapLines
              lineNumberStyle={{
                minWidth: '20px',
                paddingRight: '10px',
                marginRight: 0,
                color: 'color-mix(in srgb, var(--color-text-3, #86909c) 60%, transparent)',
                fontSize: '10.5px',
                lineHeight: 1.86,
                textAlign: 'right',
                userSelect: 'none',
              }}
              lineProps={(lineNumber: number) => ({
                style: {
                  display: 'block',
                  minWidth: 0,
                  ...(isDiff ? getDiffLineStyle(diffLines[lineNumber - 1] || '', isDark) : {}),
                },
              })}
              customStyle={{
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
                fontSize: '11.5px',
                lineHeight: 1.7,
                whiteSpace: 'pre-wrap',
              }}
              codeTagProps={{
                style: {
                  color: 'inherit',
                  background: 'transparent',
                  display: 'block',
                  maxWidth: '100%',
                  minWidth: 0,
                  overflow: 'visible',
                  overflowWrap: 'anywhere',
                  whiteSpace: 'pre-wrap',
                  wordBreak: 'break-word',
                  fontSize: '11.5px',
                  lineHeight: 1.7,
                },
              }}
            />
          </div>
        }
        footer={
          canCollapse && !isStreaming ? (
            <button
              type='button'
              id={footerId}
              aria-expanded={expanded}
              aria-controls={codeContentId}
              aria-label={
                expanded
                  ? t('common.collapse')
                  : t('common.viewMoreLines', { count: totalLines - PREVIEW_LINES })
              }
              className='markdown-code-footer'
              onClick={toggleExpanded}
            >
              <span className='markdown-code-footer-label'>
                {expanded ? t('common.collapse') : t('common.viewMoreLines', { count: totalLines - PREVIEW_LINES })}
              </span>
              {expanded ? (
                <ChevronUp size={12} strokeWidth={1.75} aria-hidden />
              ) : (
                <ChevronDown size={12} strokeWidth={1.75} aria-hidden />
              )}
            </button>
          ) : null
        }
        children={formattedContent}
      />
    </div>
  );
}

export default CodeBlock;
