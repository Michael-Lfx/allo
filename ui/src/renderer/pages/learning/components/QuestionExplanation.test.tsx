/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

import QuestionExplanation, { hasBlockLevelMarkup } from './QuestionExplanation';

describe('判分解析通道分流（hasBlockLevelMarkup）', () => {
  test('单段短文本走内联', () => {
    expect(hasBlockLevelMarkup('因为 $x^2$ 在正区间单调递增。')).toBe(false);
    expect(hasBlockLevelMarkup('正确。')).toBe(false);
    expect(hasBlockLevelMarkup('速度是矢量，速度大小是标量，二者不可混谈。')).toBe(false);
  });

  test('块级构造走块级：围栏/多段/标题/列表/引用/表格/展示公式', () => {
    expect(hasBlockLevelMarkup('见下图：\n\n```svg\n<svg></svg>\n```')).toBe(true);
    expect(hasBlockLevelMarkup('第一段。\n\n第二段。')).toBe(true);
    expect(hasBlockLevelMarkup('要点如下：\n- 甲\n- 乙')).toBe(true);
    expect(hasBlockLevelMarkup('1. 先看定义\n2. 再看反例')).toBe(true);
    expect(hasBlockLevelMarkup('# 小标题\n正文')).toBe(true);
    expect(hasBlockLevelMarkup('> 引用原文')).toBe(true);
    expect(hasBlockLevelMarkup('| 甲 | 乙 |\n| --- | --- |\n| 1 | 2 |')).toBe(true);
    expect(hasBlockLevelMarkup('质能方程：\n$$E = mc^2$$')).toBe(true);
  });

  test('行中符号不误判（竖线/大于号不在行首）', () => {
    expect(hasBlockLevelMarkup('比较 $|x|$ 与 $y$ 的大小，结果 > 预期值。')).toBe(false);
    expect(hasBlockLevelMarkup('推荐方案 A 或方案 B。')).toBe(false);
  });
});

describe('QuestionExplanation 通道选择（真实渲染，bun test 无 DOM 下 SSR）', () => {
  test('短文本 → InlineMarkdown 内联通道（原文可见，无 shadow 包裹）', () => {
    const markup = renderToStaticMarkup(<QuestionExplanation text="因为 $x^2$ 单调递增。" />);
    expect(markup).toContain('data-testid="inline-markdown"');
    expect(markup).toContain('单调递增。');
    expect(markup).not.toContain('relative w-full');
  });

  test('含围栏 → 块级 <Markdown> 通道（不进内联通道）', () => {
    const markup = renderToStaticMarkup(
      <QuestionExplanation text={'见下图：\n\n```svg\n<svg></svg>\n```'} />
    );
    // 块级通道的 SSR 包裹（ShadowView 挂载后才注入正文，bun test 无 DOM 断言外壳即可）
    expect(markup).toContain('relative w-full');
    expect(markup).not.toContain('data-testid="inline-markdown"');
  });
});
