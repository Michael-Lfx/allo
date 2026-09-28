import { ChevronDown, ChevronUp } from 'lucide-react';
import katex from 'katex';
import React, { useCallback, useDeferredValue, useEffect, useId, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import BeautifulUiCodeBlock from '@renderer/components/beautifulUi/codeBlock/CodeBlock';
import { filenameFromFenceNode } from '@renderer/components/beautifulUi/codeBlock/codeBlockLanguage';
import { CODE_LINE_HEIGHT_PX } from './codeFenceLayout';
import { formatCode } from './markdownUtils';
import ShikiCodeFence from './ShikiCodeFence';
import { resolveSyntaxLanguage } from './syntaxLanguage';

const PREVIEW_LINES = 3;
const CODE_LINE_HEIGHT = CODE_LINE_HEIGHT_PX;
const CODE_PADDING_VERTICAL = 20;
const COLLAPSED_HEIGHT = PREVIEW_LINES * CODE_LINE_HEIGHT + CODE_PADDING_VERTICAL;
const EMPTY_DIFF_LINES: string[] = [];
const INLINE_CODE_STYLE: React.CSSProperties = {
  fontFamily: 'var(--code-font, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace)',
  background: 'var(--code-bg)',
  padding: '2px 6px',
  borderRadius: 4,
};

const MermaidBlock = React.lazy(() => import('./MermaidBlock'));
const SvgBlock = React.lazy(() => import('./SvgBlock'));
const JsxGraphBlock = React.lazy(() => import('./JsxGraphBlock'));

type CodeBlockProps = {
  children: string;
  className?: string;
  node?: unknown;
  hiddenCodeCopyButton?: boolean;
  codeStyle?: React.CSSProperties;
  isStreaming?: boolean;
  [key: string]: unknown;
};

type CodeFenceHighlightProps = {
  content: string;
  language: string;
  isDiff: boolean;
  isDark: boolean;
  isStreaming: boolean;
  diffLines: string[];
};

/** Isolated from expand/collapse chrome so toggling max-height does not rebuild the token tree. */
const CodeFenceHighlight = React.memo(function CodeFenceHighlight({
  content,
  language,
  isDiff,
  isDark,
  isStreaming,
  diffLines,
}: CodeFenceHighlightProps) {
  return (
    <ShikiCodeFence
      content={content}
      language={language}
      isStreaming={isStreaming}
      isDiff={isDiff}
      isDark={isDark}
      diffLines={diffLines}
      showLineNumbers
    />
  );
});

function areCodeBlockPropsEqual(prev: CodeBlockProps, next: CodeBlockProps): boolean {
  return (
    prev.children === next.children &&
    prev.className === next.className &&
    prev.isStreaming === next.isStreaming &&
    prev.hiddenCodeCopyButton === next.hiddenCodeCopyButton &&
    prev.codeStyle === next.codeStyle
  );
}

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

  const formattedContent = useMemo(() => formatCode(children), [children]);
  const deferredContent = useDeferredValue(formattedContent);
  const highlightContent = isStreaming ? deferredContent : formattedContent;
  const match = /(?:^|\s)language-([^\s]+)/.exec(className || '');
  const language = match?.[1] || 'text';
  const highlightLanguage = resolveSyntaxLanguage(language);
  const isDiff = highlightLanguage === 'diff';
  const isDark = currentTheme === 'dark';
  const diffLines = useMemo(
    () => (isDiff ? formattedContent.split('\n') : EMPTY_DIFF_LINES),
    [isDiff, formattedContent]
  );

  const setContentRef = useCallback((node: HTMLDivElement | null) => {
    contentRef.current = node;
  }, []);

  useEffect(() => {
    if (!isStreaming) return;
    const node = contentRef.current;
    if (node) node.scrollTop = node.scrollHeight;
  }, [highlightContent, isStreaming]);

  const toggleExpanded = () => {
    const willCollapse = expanded;
    setExpanded((v) => !v);
    if (willCollapse && containerRef.current) {
      requestAnimationFrame(() => {
        containerRef.current?.scrollIntoView({ block: 'nearest', behavior: 'auto' });
      });
    }
  };

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
    return (
      <React.Suspense fallback={<div className='markdown-mermaid-loading' aria-busy='true' />}>
        <MermaidBlock code={formatCode(children)} style={props.codeStyle} />
      </React.Suspense>
    );
  }

  // Lesson figures: sanitized inline SVG (SMIL animations run natively) and
  // JSXGraph boards for interactive / programmatically animated diagrams.
  if (language === 'svg') {
    return (
      <React.Suspense fallback={<div className='markdown-mermaid-loading' aria-busy='true' />}>
        <SvgBlock code={formatCode(children)} style={props.codeStyle} />
      </React.Suspense>
    );
  }

  if (language === 'jsxgraph') {
    return (
      <React.Suspense fallback={<div className='markdown-mermaid-loading' aria-busy='true' />}>
        <JsxGraphBlock code={formatCode(children)} style={props.codeStyle} />
      </React.Suspense>
    );
  }

  // Inline code (single line)
  if (!String(children).includes('\n')) {
    return (
      <code {...rest} className={className} style={INLINE_CODE_STYLE}>
        {children}
      </code>
    );
  }

  const totalLines = formattedContent.split('\n').length;
  const canCollapse = totalLines > PREVIEW_LINES;
  const isEffectivelyExpanded = isStreaming || expanded;

  const codeContentId = `${blockId}-content`;
  const footerId = `${blockId}-footer`;

  return (
    <div
      ref={containerRef}
      style={{ width: '100%', minWidth: 0, maxWidth: '100%', ...props.codeStyle }}
      className='markdown-code-block'
      data-testid='markdown-code-block'
      data-streaming={isStreaming ? 'true' : 'false'}
      data-collapsible={canCollapse ? 'true' : 'false'}
      data-collapse-state={!canCollapse ? 'none' : isEffectivelyExpanded ? 'expanded' : 'collapsed'}
    >
      <BeautifulUiCodeBlock
        language={language}
        filename={filename}
        streaming={false}
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
            ref={setContentRef}
            id={codeContentId}
            className='markdown-code-content'
            style={{
              maxHeight: canCollapse && !isEffectivelyExpanded ? `${COLLAPSED_HEIGHT}px` : 'none',
              overflowX: 'clip',
              overflowY: isStreaming ? 'auto' : 'clip',
            }}
          >
            <CodeFenceHighlight
              content={highlightContent}
              language={highlightLanguage}
              isDiff={isDiff}
              isDark={isDark}
              isStreaming={isStreaming}
              diffLines={diffLines}
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
              data-testid='markdown-code-footer'
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

export default React.memo(CodeBlock, areCodeBlockPropsEqual);
