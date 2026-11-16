/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import Markdown, { InlineMarkdown } from '@renderer/components/Markdown';
import React from 'react';

/**
 * 行首块级构造（含 ```svg fence / 标题 / 列表 / 引用 / 表格 / 展示公式）：
 * 命中即走块级通道，否则按单段短文本走内联通道。
 */
const BLOCK_LINE_RE = /(^|\n)[ \t]{0,3}(?:```|~~~|#{1,6}[ \t]|>|[-*+][ \t]|\d{1,9}[.)][ \t]|\||\$\$)/;

/**
 * 判分解析/答错揭示的通道分流谓词（课时练习面与复习面共用，口径统一——
 * 同一份 explanation 在两处渲染形态一致）。含块级构造（如 ```svg fence，
 * 纯文本渲染会露源码）或多段解析走块级 <Markdown>（与课时正文同管线）；
 * 单段短文本走 InlineMarkdown 内联通道。
 */
export function hasBlockLevelMarkup(text: string): boolean {
  // 空行分段（\n\n）= 多段解析，块级通道才有合法的段落文档流
  return /\n[ \t]*\n/.test(text) || BLOCK_LINE_RE.test(text);
}

/** 题面解析/反馈文本的统一渲染入口：块级 ↔ 内联按上面谓词分流。 */
const QuestionExplanation: React.FC<{ text: string }> = ({ text }) =>
  hasBlockLevelMarkup(text) ? <Markdown>{text}</Markdown> : <InlineMarkdown text={text} />;

export { QuestionExplanation };
export default QuestionExplanation;
