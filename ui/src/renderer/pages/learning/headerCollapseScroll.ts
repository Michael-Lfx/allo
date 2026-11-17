/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 图工作区头部滚动自动折叠（a1）的纯决策层。零 React 零 DOM——输入正文
 * 滚动深度与临时层现态，输出下一态；组件只做接线（headerCollapse.ts 同款
 * 分层纪律：决策面可测，接线面走 render 测试）。语义（a1 裁决 R-a/R-b，
 * processing 否决行已被 a3 删除——processing 不进本状态机）：
 * - persisted=true：用户偏好恒折叠，临时层语义被吞、否决臂复位；
 * - scrollTop ≥ H：临时折叠；< H/2：临时展开 + 否决臂复位（H/2 迟滞带
 *   [H/2, H) 维持现态，防临界抖动）；
 * - 否决臂（override）：临时折叠中用户手动展开后由组件置位，本滚动周期
 *   压制自动折叠，滚回顶部阈值区（< H/2）由本机复位。
 * 临时层**不落盘**：滚动深度是视口语境不是偏好——存储层（headerCollapse.ts）
 * 只承载手动 chevron 的 f3 持久语义，本层零存储接触。 */

import {
  HEADER_COLLAPSE_SCROLL_COLLAPSE_PX,
  HEADER_COLLAPSE_SCROLL_EXPAND_PX,
} from './constants';

/** 状态机输入：滚动深度 + 持久层标记 + 临时层现态（折叠 / 否决臂）。 */
export interface HeaderScrollCollapseState {
  scrollTop: number;
  persisted: boolean;
  collapsed: boolean;
  override: boolean;
}

/** 状态机输出：临时层下一态（有效折叠 = persisted ∪ collapsed）。 */
export interface HeaderScrollCollapseOutput {
  collapsed: boolean;
  override: boolean;
}

export function resolveHeaderScrollCollapse(
  input: HeaderScrollCollapseState
): HeaderScrollCollapseOutput {
  const { scrollTop, persisted, collapsed, override } = input;
  // 偏好恒折叠：持久层在场吞掉临时层语义，否决臂一并复位（偏好接管）。
  // 边界（F4 审查裁决钉，接线测试见 LearningGraphWorkspace.header-collapse
  // .test.tsx「persisted 深滚态手动展开同样进否决臂」）：本臂只在**偏好仍
  // 在场**时生效。用户在 persisted 深滚态手动展开走组件 collapseHeader 的
  // 展开臂（持久层清条目 + 否决臂置位）——persisted 退场后本臂不再吞臂，
  // 同深滚（≥ H）不被再折叠；滚回 < H/2 由回顶臂复位，自动层照常。
  if (persisted) return { collapsed: true, override: false };
  // 回顶（< H/2 迟滞下界）：临时展开，否决臂复位——新滚动周期开始。
  if (scrollTop < HEADER_COLLAPSE_SCROLL_EXPAND_PX) {
    return { collapsed: false, override: false };
  }
  // 下滑过阈值（≥ H）：否决臂在场则本周期不再自动折叠，否则临时折叠。
  if (scrollTop >= HEADER_COLLAPSE_SCROLL_COLLAPSE_PX) {
    return override ? { collapsed, override } : { collapsed: true, override };
  }
  // 迟滞带 [H/2, H)：维持现态。
  return { collapsed, override };
}
