/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 头部滚动自动折叠状态机（a1）表驱动单测：阈值 / 迟滞 / 否决臂 / persisted
 * 恒折叠全分支 + 完整滚动周期走查。纯函数零 React 零 DOM——输入滚动深度与
 * 现态，输出下一态（a1 R-c 分层：决策面在此，接线面在 render 测试侧）。
 * 存储层零改动断言在 LearningGraphWorkspace.header-collapse.test.tsx（临时
 * 层不落盘是组件接线性质，纯函数面天然无存储）。 */
import { describe, expect, test } from 'bun:test';

import {
  HEADER_COLLAPSE_SCROLL_COLLAPSE_PX,
  HEADER_COLLAPSE_SCROLL_EXPAND_PX,
} from './constants';
import { resolveHeaderScrollCollapse } from './headerCollapseScroll';

interface MachineCase {
  name: string;
  scrollTop: number;
  persisted?: boolean;
  collapsed?: boolean;
  override?: boolean;
  out: { collapsed: boolean; override: boolean };
}

const CASES: MachineCase[] = [
  // 阈值臂：下滑过 H → 临时折叠
  { name: '未滚动（0）维持展开', scrollTop: 0, out: { collapsed: false, override: false } },
  {
    name: '恰在折叠阈值（160）临时折叠',
    scrollTop: 160,
    out: { collapsed: true, override: false },
  },
  {
    name: '越过阈值（200）临时折叠',
    scrollTop: 200,
    out: { collapsed: true, override: false },
  },
  // 迟滞带 [H/2, H)：维持现态（防临界抖动）
  {
    name: '迟滞带（120）维持展开现态',
    scrollTop: 120,
    out: { collapsed: false, override: false },
  },
  {
    name: '迟滞带（120）维持折叠现态',
    scrollTop: 120,
    collapsed: true,
    out: { collapsed: true, override: false },
  },
  {
    name: '迟滞带下界（80）不算回顶（<80 才复位）',
    scrollTop: 80,
    collapsed: true,
    override: true,
    out: { collapsed: true, override: true },
  },
  // 回顶臂：< H/2 → 临时展开 + 否决臂复位
  {
    name: '回顶（79）展开并复位否决臂',
    scrollTop: 79,
    collapsed: true,
    override: true,
    out: { collapsed: false, override: false },
  },
  {
    name: '顶部（0）复位否决臂',
    scrollTop: 0,
    override: true,
    out: { collapsed: false, override: false },
  },
  // 否决臂：override 在场时下滑不再自动折叠（用户否决本滚动周期）
  {
    name: '否决臂在场：越阈值维持展开不折叠',
    scrollTop: 300,
    override: true,
    out: { collapsed: false, override: true },
  },
  {
    name: '否决臂在场：迟滞带维持折叠现态',
    scrollTop: 100,
    collapsed: true,
    override: true,
    out: { collapsed: true, override: true },
  },
  // persisted 恒折叠（尊重偏好，a1 R-b 矩阵首行）
  {
    name: 'persisted 恒折叠：顶部也折叠',
    scrollTop: 0,
    persisted: true,
    out: { collapsed: true, override: false },
  },
  {
    name: 'persisted 恒折叠：深滚动折叠且否决臂复位',
    scrollTop: 500,
    persisted: true,
    override: true,
    out: { collapsed: true, override: false },
  },
];

describe('头部滚动自动折叠状态机（a1）', () => {
  for (const c of CASES) {
    test(`表驱动：${c.name}`, () => {
      expect(
        resolveHeaderScrollCollapse({
          scrollTop: c.scrollTop,
          persisted: c.persisted ?? false,
          collapsed: c.collapsed ?? false,
          override: c.override ?? false,
        })
      ).toEqual(c.out);
    });
  }

  test('常量契约：H=160，迟滞回展阈值为 H/2', () => {
    expect(HEADER_COLLAPSE_SCROLL_COLLAPSE_PX).toBe(160);
    expect(HEADER_COLLAPSE_SCROLL_EXPAND_PX).toBe(HEADER_COLLAPSE_SCROLL_COLLAPSE_PX / 2);
  });

  test('完整周期：滚过阈值折叠 → 用户否决展开 → 深滚不再折叠 → 回顶复位 → 新周期恢复自动臂', () => {
    const at = (scrollTop: number, state: { collapsed: boolean; override: boolean }) =>
      resolveHeaderScrollCollapse({ scrollTop, persisted: false, ...state });
    let state = at(200, { collapsed: false, override: false });
    expect(state).toEqual({ collapsed: true, override: false });
    // 用户在 auto 折叠态点展开（组件置否决臂），状态机此后接手
    state = at(300, { collapsed: false, override: true });
    expect(state).toEqual({ collapsed: false, override: true });
    state = at(120, state);
    expect(state).toEqual({ collapsed: false, override: true });
    state = at(50, state);
    expect(state).toEqual({ collapsed: false, override: false });
    state = at(200, state);
    expect(state).toEqual({ collapsed: true, override: false });
  });
});
