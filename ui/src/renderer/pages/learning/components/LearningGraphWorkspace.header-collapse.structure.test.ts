/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 图工作区折叠钮显性化（a2 重修）的接线面断言：两钮 icon+text 文字标
 * 签（可见文本根治发现性——真机 FAIL 教训：裸图标/Tooltip 悬停不可发
 * 现）、切换钮统一行尾右侧（展开态收起钮在行尾控件排、折叠态展开钮在
 * 摘要行尾，位置规则「切换恒在行尾」）。用 structure test 读源码断言
 * 而非渲染断言——bun test 宿主 import 提升先于 jsdom globals 注入，
 * arco isServerRendering 在模块加载期恒 true → portal 弹层永不渲染
 * （AutoTierSelector / AgentTraceInspector structure test 同款先例）。
 * DOM 面（文字/点击往返/否决臂）由 LearningGraphWorkspace.header-collapse.
 * test.tsx 渲染测试覆盖。 */
import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

describe('LearningGraphWorkspace 折叠钮显性化（a2 重修）structure', () => {
  const source = readFileSync(
    new URL('./LearningGraphWorkspace.tsx', import.meta.url),
    'utf8'
  );

  test('两钮 icon+text 文字标签复用 aria 同源词条（零新键）', () => {
    // 展开态收起钮：文字标签 + aria 同键（icon+text 模式）
    expect(source.includes("aria-label={t('learning.learningGraphHeaderCollapse')}")).toBe(true);
    expect(
      source.includes(
        "<IconDown className='shrink-0 text-14px' />\n              {t('learning.learningGraphHeaderCollapse')}"
      )
    ).toBe(true);
    // 折叠态展开钮：同款 icon+text
    expect(source.includes("aria-label={t('learning.learningGraphHeaderExpand')}")).toBe(true);
    expect(
      source.includes(
        "<IconRight className='shrink-0 text-14px' />\n            {t('learning.learningGraphHeaderExpand')}"
      )
    ).toBe(true);
  });

  test('文字钮不再依赖 Tooltip 悬停（真机 FAIL：裸图标/悬停不可发现）', () => {
    expect(
      source.includes("<Tooltip content={t('learning.learningGraphHeaderCollapse')}>")
    ).toBe(false);
    expect(
      source.includes("<Tooltip content={t('learning.learningGraphHeaderExpand')}>")
    ).toBe(false);
    expect(source.includes('!px-2px')).toBe(false);
    expect(source.includes("<IconDown className='text-12px' />")).toBe(false);
    expect(source.includes("<IconRight className='shrink-0 text-12px' />")).toBe(false);
  });

  test('切换钮统一行尾右侧：收起钮在行尾控件排（展开态）、展开钮在摘要行尾（折叠态）', () => {
    // 展开态：收起钮位于右端控件排内（restore/改名/宽度/模型排之首），
    // 且不再出现在标题左侧（标题行内无 IconDown）。
    const expandedRow = source.slice(
      source.indexOf("className='flex shrink-0 items-center gap-8px'"),
      source.indexOf('{/* 知识结构状态 + 补充节点（生长控制条） */}')
    );
    expect(expandedRow.includes('learningGraphHeaderCollapse')).toBe(true);
    const titleRow = source.slice(
      source.indexOf('<div className=\'flex items-center gap-8px\'>'),
      source.indexOf("{/* f3 Q6 目标段去重")
    );
    expect(titleRow.includes('IconDown')).toBe(false);
    // 折叠态：展开钮是摘要行最后一个元素（行尾——在摘要文本之后）。
    const collapsedBar = source.slice(
      source.indexOf("className='flex shrink-0 flex-wrap items-center gap-8px'"),
      source.indexOf('{/* 知识检测入口')
    );
    expect(collapsedBar.indexOf('learningGraphHeaderExpand')).toBeGreaterThan(
      collapsedBar.indexOf('headerSummaryText')
    );
    expect(collapsedBar.indexOf('learningGraphHeaderExpand')).toBeGreaterThan(
      collapsedBar.indexOf('processingBadge')
    );
  });

  test('折叠态：课程名转纯文本 truncate span（不再嵌在展开钮内）', () => {
    expect(source.includes('min-w-0 max-w-360px truncate text-[var(--color-text-1)]')).toBe(true);
  });
});
