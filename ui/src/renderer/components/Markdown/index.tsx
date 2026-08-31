

import ReactMarkdown, { defaultUrlTransform } from 'react-markdown';
import type { Components, ExtraProps, UrlTransform } from 'react-markdown';

import rehypeKatex from 'rehype-katex';
import rehypeRaw from 'rehype-raw';
import remarkBreaks from 'remark-breaks';
import remarkGfm from 'remark-gfm';
import remarkMath from 'remark-math';

// Import KaTeX CSS to make it available in the document
import 'katex/dist/katex.min.css';

import { openExternalUrl } from '@/renderer/utils/platform';
import classNames from 'classnames';
import React, { useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { convertLatexDelimiters } from '@renderer/utils/chat/latexDelimiters';
import LocalImageView from '@renderer/components/media/LocalImageView';
import { isLocalImageSource } from '@/common/utils/localPath';
import CodeBlock from './CodeBlock';
import CollapsibleBlockquote from './CollapsibleBlockquote';
import ShadowView from './ShadowView';
import { shouldCollapseMarkdownBlockquote } from './blockquoteCollapse';
import type { MarkdownViewProps } from './markdownViewProps';
import { remarkSafeBreaks } from './remarkSafeBreaks';

export type { MarkdownViewProps } from './markdownViewProps';

const REMARK_PLUGINS = [remarkGfm, remarkMath, remarkBreaks, remarkSafeBreaks];

const markdownUrlTransform: UrlTransform = (url, key, node) => {
  if (key === 'src' && node.tagName === 'img') {
    // react-markdown rejects unknown schemes by default. Permit filesystem
    // references for LocalImageView and inline/blob images for the browser,
    // while leaving every other protocol on the library's safe allowlist.
    if (isLocalImageSource(url) || /^data:image\//i.test(url) || /^blob:/i.test(url)) {
      return url;
    }
  }
  return defaultUrlTransform(url);
};

const COMPACT_FONT_SIZE = '14px';
const COMPACT_LINE_HEIGHT = '22px';

const MarkdownView: React.FC<MarkdownViewProps> = React.memo(
  ({
    hiddenCodeCopyButton,
    codeStyle,
    className,
    onRef,
    fontSize: fontSizeProp,
    lineHeight: lineHeightProp,
    compact = false,
    allowHtml,
    allowUnverifiedImages = true,
    isStreaming = false,
    collapsibleBlockquotes = false,
    children: childrenProp,
  }) => {
    const { t } = useTranslation();

    // `compact` is the document-preview density; explicit props still win.
    const fontSize = fontSizeProp ?? (compact ? COMPACT_FONT_SIZE : undefined);
    const lineHeight = lineHeightProp ?? (compact ? COMPACT_LINE_HEIGHT : undefined);

    const normalizedChildren = useMemo(() => {
      if (typeof childrenProp === 'string') {
        return convertLatexDelimiters(childrenProp);
      }
      return childrenProp;
    }, [childrenProp]);

    const handleLinkClick = useCallback(
      (e: React.MouseEvent<HTMLAnchorElement>) => {
        e.preventDefault();
        e.stopPropagation();
        const href = (e.currentTarget as HTMLAnchorElement).href;
        if (!href) return;
        openExternalUrl(href).catch((error: unknown) => {
          console.error(t('messages.openLinkFailed'), error);
        });
      },
      [t]
    );

    // Memoize components so React preserves component identity across re-renders.
    // Without this, every streaming update creates new function references → React
    // unmounts/remounts all custom components → hooks & DOM state are lost.
    const components = useMemo<Components>(
      () => ({
        span: ({ node: _node, className: cn, children: ch, ...rest }: React.JSX.IntrinsicElements['span'] & ExtraProps) => (
          <span {...rest} className={cn}>
            {ch}
          </span>
        ),
        code: (props: React.JSX.IntrinsicElements['code'] & ExtraProps) => (
          <CodeBlock
            {...(props as Parameters<typeof CodeBlock>[0])}
            codeStyle={codeStyle}
            hiddenCodeCopyButton={hiddenCodeCopyButton}
            isStreaming={isStreaming}
          />
        ),
        a: ({ node: _node, ...rest }: React.JSX.IntrinsicElements['a'] & ExtraProps) => (
          <a {...rest} target='_blank' rel='noreferrer' onClick={handleLinkClick} />
        ),
        table: ({ node: _node, ...rest }: React.JSX.IntrinsicElements['table'] & ExtraProps) => (
          <div className='markdown-table-wrap'>
            <table {...rest} />
          </div>
        ),
        blockquote: ({ node: blockquoteNode, children, ...rest }: React.JSX.IntrinsicElements['blockquote'] & ExtraProps) => {
          const canCollapse = collapsibleBlockquotes && shouldCollapseMarkdownBlockquote(blockquoteNode);

          return (
            canCollapse ? (
              <CollapsibleBlockquote {...rest}>{children}</CollapsibleBlockquote>
            ) : (
              <blockquote {...rest}>{children}</blockquote>
            )
          );
        },
        img: ({ node: _node, ...rest }: React.JSX.IntrinsicElements['img'] & ExtraProps) => {
          const imgProps = rest;
          const src = imgProps.src || '';
          if (!allowUnverifiedImages) {
            return (
              <span className={imgProps.className}>
                Unverified image reference: {imgProps.alt || src || 'unknown source'}
              </span>
            );
          }
          if (isLocalImageSource(src)) {
            return <LocalImageView src={src} alt={imgProps.alt || ''} className={imgProps.className} />;
          }
          return <img {...imgProps} />;
        },
      }),
      [allowUnverifiedImages, codeStyle, collapsibleBlockquotes, hiddenCodeCopyButton, handleLinkClick, isStreaming]
    );

    const rehypePlugins = useMemo(() => (allowHtml ? [rehypeRaw, rehypeKatex] : [rehypeKatex]), [allowHtml]);

    return (
      <div className={classNames('relative w-full', className)}>
        <ShadowView fontSize={fontSize} lineHeight={lineHeight}>
          <div ref={onRef} className='markdown-shadow-body'>
            <ReactMarkdown
              remarkPlugins={REMARK_PLUGINS}
              rehypePlugins={rehypePlugins}
              components={components}
              urlTransform={markdownUrlTransform}
            >
              {normalizedChildren}
            </ReactMarkdown>
          </div>
        </ShadowView>
      </div>
    );
  }
);

MarkdownView.displayName = 'MarkdownView';

export default MarkdownView;
