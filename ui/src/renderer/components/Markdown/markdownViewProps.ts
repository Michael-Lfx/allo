import type React from 'react';

export type MarkdownViewProps = {
  children: string;
  hiddenCodeCopyButton?: boolean;
  codeStyle?: React.CSSProperties;
  className?: string;
  onRef?: (el?: HTMLDivElement | null) => void;
  fontSize?: string;
  lineHeight?: string;
  /** Document-density typography for preview surfaces (prompt/knowledge previews). */
  compact?: boolean;
  /** Enable raw HTML rendering in markdown content. Use with caution — only for trusted sources. */
  allowHtml?: boolean;
  /** Model/tool Markdown is not a verified artifact-delivery receipt. */
  allowUnverifiedImages?: boolean;
  /** When true, code blocks stay expanded and tail-follow during streaming. */
  isStreaming?: boolean;
};
