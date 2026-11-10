/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** 学习域长任务的进程级在途注册（d6/d7）：组件本地 state 随路由卸载即丢，
 * 切页往返后按钮恢复可点、再点被后端槽锁 409。本模块把「发起时标记、落定
 * 时清除」的标记放进进程级 Map——同会话内组件重挂载后读它即可恢复在途态。
 * 跨会话（后端重启/页面刷新）的恢复走 `GET /api/learning/generation-tasks`
 * 快照（服务端真相，d6 起 basis_declare 也在内）；两路取并集。 */

const pending = new Map<string, number>();

/** 标记一个在途操作（发起时调用）。 */
export function markInFlight(key: string): void {
  pending.set(key, Date.now());
}

/** 清除在途标记（操作落定——成功或失败——时调用）。 */
export function clearInFlight(key: string): void {
  pending.delete(key);
}

/** 是否在途（同会话内有效；跨会话配合 tasks 快照取并集）。 */
export function isInFlight(key: string): boolean {
  return pending.has(key);
}

/** 某前缀下在途的 key 尾段集合（如前缀 `lesson:` → 课程内生成中的 lessonId
 * 集）。 */
export function inFlightIdsWithPrefix(prefix: string): Set<string> {
  const ids = new Set<string>();
  for (const key of pending.keys()) {
    if (key.startsWith(prefix)) {
      ids.add(key.slice(prefix.length));
    }
  }
  return ids;
}
