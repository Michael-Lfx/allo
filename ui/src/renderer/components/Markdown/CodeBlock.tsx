

import { Message } from '@arco-design/web-react';
import { Copy, Down, Up } from '@icon-park/react';
import katex from 'katex';
import React, { useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { copyText } from '@/renderer/utils/ui/clipboard';
import MermaidBlock from './MermaidBlock';
import { formatCode, getDiffLineStyle } from './markdownUtils';
import SyntaxHighlighter, { vs, vs2015 } from './SyntaxHighlighter';

const PREVIEW_LINES = 3;
// code span: font-size 13px, line-height 20px (per ShadowView injection)
const CODE_LINE_HEIGHT = 20;
// SyntaxHighlighter pre padding: 0.5em top + 0.5em bottom ≈ 13px each side
const CODE_PADDING_VERTICAL = 13;
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

  const { children, className, node: _node, hiddenCodeCopyButton, codeStyle: _c, isStreaming = false, ...rest } = props;
  const match = /language-(\w+)/.exec(className || '');
  const language = match?.[1] || 'text';

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
  const codeTheme = currentTheme === 'dark' ? vs2015 : vs;
  const diffLines = isDiff ? formattedContent.split('\n') : [];
  const isDark = currentTheme === 'dark';

  const handleCopy = () => {
    void copyText(formattedContent)
      .then(() => {
        try {
          Message.success(t('common.copySuccess'));
        } catch {
          /* Shadow DOM portal may fail silently */
        }
      })
      .catch(() => {
        try {
          Message.error(t('common.copyFailed'));
        } catch {
          /* ignore */
        }
      });
  };

  const codeContentId = `${blockId}-content`;
  const footerId = `${blockId}-footer`;

  return (
    <div
      ref={containerRef}
      style={{ width: '100%', minWidth: 0, maxWidth: '100%', ...props.codeStyle }}
      className='markdown-code-block'
    >
      <div className='markdown-code-surface'>
        {/* Header */}
        <div className='markdown-code-header'>
          <span className='markdown-code-language'>
            {language.toLocaleLowerCase()}
          </span>
          {/* Buttons: always visible on touch devices, hover-only on pointer devices */}
          <div
            className='markdown-code-toolbar'
          >
            {canCollapse && !isStreaming && (
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
                  <Up theme='outline' size='14' style={{ display: 'block' }} fill='currentColor' />
                ) : (
                  <Down theme='outline' size='14' style={{ display: 'block' }} fill='currentColor' />
                )}
              </button>
            )}
            {!hiddenCodeCopyButton && (
              <button
                type='button'
                title={t('common.copy')}
                aria-label={t('common.copy')}
                className='markdown-code-action'
                onClick={handleCopy}
              >
                <Copy theme='outline' size='14' style={{ display: 'block' }} fill='currentColor' />
              </button>
            )}
          </div>
        </div>

        {/* Code content — always full content, clipped by maxHeight when collapsed */}
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
            overflowY: isStreaming ? 'auto' : 'hidden',
            overflowX: 'visible',
          }}
        >
          <SyntaxHighlighter
            children={formattedContent}
            language={language}
            style={codeTheme}
            PreTag='div'
            wrapLines={isDiff}
            lineProps={
              isDiff
                ? (lineNumber: number) => ({
                    style: {
                      display: 'block',
                      ...getDiffLineStyle(diffLines[lineNumber - 1] || '', isDark),
                    },
                  })
                : undefined
            }
            customStyle={{
              margin: 0,
              padding: '0 12px 8px',
              borderRadius: 0,
              border: 'none',
              background: 'transparent',
              color: 'var(--text-primary)',
              overflowX: 'auto',
              maxWidth: '100%',
            }}
            codeTagProps={{
              style: {
                color: 'var(--text-primary)',
                background: 'transparent',
              },
            }}
          />
        </div>

        {/* Footer */}
        {canCollapse && !isStreaming && (
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
              <Up theme='outline' size='12' fill='currentColor' />
            ) : (
              <Down theme='outline' size='12' fill='currentColor' />
            )}
          </button>
        )}
      </div>
    </div>
  );
}

export default CodeBlock;
