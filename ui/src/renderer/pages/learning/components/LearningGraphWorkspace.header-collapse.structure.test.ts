/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 图工作区折叠钮显性化（a2）的接线面断言：Tooltip 复用 aria 同源词条、
 * hit area 常规化、图标 14px。用 structure test 读源码断言而非渲染断言——
 * bun test 宿主 import 提升先于 jsdom globals 注入，arco isServerRendering
 * 在模块加载期恒 true → portal 弹层永不渲染（AutoTierSelector / 
 * AgentTraceInspector structure test 同款先例：Tooltip 断言走源码面）。
 * DOM 面（hit area 类 / 图标类 / 摘要条钮序 / 点击往返）由
 * LearningGraphWorkspace.header-collapse.test.tsx 渲染测试覆盖。 */
import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

describe('LearningGraphWorkspace 折叠钮显性化（a2）structure', () => {
  const source = readFileSync(
    new URL('./LearningGraphWorkspace.tsx', import.meta.url),
    'utf8'
  );

  test('两钮 Tooltip 接线复用 aria 同源词条（零新键）', () => {
    // 展开态收起钮：Tooltip content 与 aria-label 同键
    expect(source.includes("<Tooltip content={t('learning.learningGraphHeaderCollapse')}>")).toBe(
      true
    );
    expect(source.includes("aria-label={t('learning.learningGraphHeaderCollapse')}")).toBe(true);
    // 折叠态展开钮：同键 Tooltip
    expect(source.includes("<Tooltip content={t('learning.learningGraphHeaderExpand')}>")).toBe(
      true
    );
    expect(source.includes("aria-label={t('learning.learningGraphHeaderExpand')}")).toBe(true);
  });

  test('hit area 常规化：全文件不再有 !px-2px 折叠钮覆写', () => {
    expect(source.includes('!px-2px')).toBe(false);
  });

  test('图标 14px：折叠/展开钮不再 12px 裸图标', () => {
    expect(source.includes("<IconDown className='text-14px' />")).toBe(true);
    expect(source.includes("<IconRight className='shrink-0 text-14px' />")).toBe(true);
    expect(source.includes("<IconDown className='text-12px' />")).toBe(false);
    expect(source.includes("<IconRight className='shrink-0 text-12px' />")).toBe(false);
  });

  test('折叠态：课程名转纯文本 truncate span（不再嵌在展开钮内）', () => {
    expect(source.includes('min-w-0 max-w-360px truncate text-[var(--color-text-1)]')).toBe(true);
  });
});
