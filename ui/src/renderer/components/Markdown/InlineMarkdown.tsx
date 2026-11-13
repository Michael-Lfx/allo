/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import ReactMarkdown from 'react-markdown';
import type { Components, ExtraProps } from 'react-markdown';
import rehypeKatex from 'rehype-katex';
import remarkGfm from 'remark-gfm';
import remarkMath from 'remark-math';

// KaTeX CSS is also imported once globally by ./index.tsx; importing it here too
// keeps this module self-sufficient (bundler dedupes the asset).
import 'katex/dist/katex.min.css';

import { openExternalUrl } from '@/renderer/utils/platform';
import React, { useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { convertLatexDelimiters } from '@renderer/utils/chat/latexDelimiters';
import { remarkSafeBreaks } from './remarkSafeBreaks';

// ── fill_in_blank 空位（allo 特有）────────────────────────────────────────
// 题面契约（generation/mod.rs）：fill_in_blank 的 prompt 恰含一个 `___`。
// 裸 `___` 进 markdown 解析器会被 GFM 吃掉——独占一行变 <hr>、成对变粗斜体。
// 解析前把下划线串换成私有区占位符，再由 rehype 插件拆成带下划线的空位 span。
const BLANK_SENTINEL = '\uE000BLANK\uE001';
const BLANK_RUN_RE = /_{3,}/g;

// 语义 token 空位样式（light-DOM，亮暗主题都走 var()）；字面量必须留在本文件
// 里，UnoCSS 才会为它们生成工具类。
const BLANK_SLOT_CLASSES =
  'inline-block h-14px min-w-48px mx-2px align-middle border-b border-b-solid border-b-[var(--color-border-2)] bg-[var(--color-fill-2)]';

const prepareInlineText = (text: string): string =>
  convertLatexDelimiters(text.replace(BLANK_RUN_RE, BLANK_SENTINEL));

type BlankHastNode = {
  type: string;
  value?: string;
  tagName?: string;
  properties?: Record<string, unknown>;
  children?: BlankHastNode[];
};

const visitBlankSentinels = (node: BlankHastNode): void => {
  const children = node.children;
  if (!children) return;

  for (let index = 0; index < children.length; index += 1) {
    const child = children[index];
    if (
      child.type === 'text' &&
      typeof child.value === 'string' &&
      child.value.includes(BLANK_SENTINEL)
    ) {
      const parts = child.value.split(BLANK_SENTINEL);
      const replacement: BlankHastNode[] = [];
      parts.forEach((part, partIndex) => {
        if (part) replacement.push({ type: 'text', value: part });
        if (partIndex < parts.length - 1) {
          replacement.push({
            type: 'element',
            tagName: 'span',
            properties: { className: BLANK_SLOT_CLASSES, 'data-blank-slot': 'true' },
            children: [],
          });
        }
      });
      children.splice(index, 1, ...replacement);
      index += replacement.length - 1;
      continue;
    }
    visitBlankSentinels(child);
  }
};

/**
 * rehype 插件：把空位占位符拆成可见的带下划线空位 span（data-blank-slot）。
 * 默认渲染即可输出 className/data 属性，无需 components 覆写。
 */
export function blankSlotHast() {
  return (tree: BlankHastNode): void => {
    visitBlankSentinels(tree);
  };
}

// 与正文（Markdown/index.tsx）同一条插件链，但两处刻意不同：
//  - 排除 remarkBreaks：题面是短语位，单换行按 markdown 口径折叠为空格，
//    不升级成 <br>；
//  - 永不启用 rehypeRaw：题面源 = 材料摘要 + 模型产物（untrusted），裸 HTML
//    默认转义是底线。
const INLINE_REMARK_PLUGINS = [remarkGfm, remarkMath, remarkSafeBreaks];
const INLINE_REHYPE_PLUGINS = [rehypeKatex, blankSlotHast];

// ── 渲染失败边界（fail-visible）──────────────────────────────────────────
// KaTeX 之外的任何渲染异常都降级为「提示 + 原文」，绝不静默丢题面。
const InlineRenderFallback = ({ text }: { text: string }) => {
  const { t } = useTranslation();
  return (
    <span data-testid='inline-markdown' className='break-all text-t-secondary'>
      <span className='text-t-tertiary'>{t('learning.inlineRenderFailed')}</span>
      {text}
    </span>
  );
};

type InlineBoundaryProps = {
  text: string;
  children: React.ReactNode;
};

/** 渲染失败边界：失败显示原文，文本变化（原地编辑）时自动复位。 */
export class InlineRenderBoundary extends React.Component<InlineBoundaryProps, { failed: boolean }> {
  state: { failed: boolean } = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  componentDidUpdate(previousProps: InlineBoundaryProps): void {
    if (this.state.failed && previousProps.text !== this.props.text) {
      this.setState({ failed: false });
    }
  }

  render(): React.ReactNode {
    return this.state.failed ? <InlineRenderFallback text={this.props.text} /> : this.props.children;
  }
}

/**
 * 题面内联渲染通道（B 线 n1）：学习模块所有题面短文本（题干/选项/排序条目/
 * 正确答案揭示）从纯文本插值切到与正文同一条 markdown + GFM + KaTeX 管线，
 * 但取**内联形态**——span 容器 + 段落解包为 span，合法嵌入 arco Radio/Checkbox
 * 的 label 短语内容位；不携带正文通道（ShadowView）钉死的排版与块级文档流。
 * 渲染失败沿 InlineRenderBoundary fail-visible。
 */
export const InlineMarkdown: React.FC<{ text: string; className?: string }> = ({ text, className }) => {
  const { t } = useTranslation();

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

  const components = useMemo<Components>(
    () => ({
      // 段落解包为 span：宿主位多是 arco label/短语内容位，禁再嵌块级 <p>。
      p: ({ node: _node, ...rest }: React.JSX.IntrinsicElements['p'] & ExtraProps) => <span {...rest} />,
      a: ({ node: _node, children, ...rest }: React.JSX.IntrinsicElements['a'] & ExtraProps) => (
        <a {...rest} target='_blank' rel='noreferrer noopener' onClick={handleLinkClick}>
          {children}
        </a>
      ),
    }),
    [handleLinkClick]
  );

  const prepared = useMemo(() => prepareInlineText(text), [text]);

  return (
    <InlineRenderBoundary text={text}>
      <span data-testid='inline-markdown' className={className}>
        <ReactMarkdown
          remarkPlugins={INLINE_REMARK_PLUGINS}
          rehypePlugins={INLINE_REHYPE_PLUGINS}
          components={components}
        >
          {prepared}
        </ReactMarkdown>
      </span>
    </InlineRenderBoundary>
  );
};
