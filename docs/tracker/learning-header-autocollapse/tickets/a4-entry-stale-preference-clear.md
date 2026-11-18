---
id: a4
title: 入口持久折叠一次性清除（存储键升版 v2）
type: task
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

真机 FAIL（2026-11-18 用户走查）：「页面进入就是折叠的，根本看不出来
滚动自动折叠，初次使用的用户根本不知道可以展开」——a1 上线后入口态仍
由 **f3 时代的持久偏好**决定（旧用户手动折叠时写下的
`learning.graph.headerCollapsed` 标记），a1 改变折叠语义后旧偏好不再代
表同一意图，老用户永远见不到展开态与自动折叠行为。如何处置？

## 已核验事实（2026-11-18 对码）

- 入口态来源：`headerCollapsed` 初始化 = `loadGraphHeaderCollapsed`
  （headerCollapse.ts，持久层 v1 键 `learning.graph.headerCollapsed`）；
  滚动临时层 `scrollAuto` 挂载即 `{collapsed:false}`——入口折叠与 a1
  无关，是 v1 旧偏好。
- f3 语义「跨会话记住手动折叠」在 a1 后变味：手动折叠钮（重修后）是
  显性文字钮，持久偏好的价值只剩「老用户残留标记」。

## Terms（施工定案）

- **R-a（落地）存储键升版 v2**：
  `GRAPH_HEADER_COLLAPSED_KEY = 'learning.graph.headerCollapsed.v2'`；
  v1 旧键作废——读侧发现 v2 缺席即顺手移除 v1（best-effort，
  removeItem 可缺省兼容旧注入方），全体用户从展开态重新起步。
- **R-b 新偏好语义不变**：v2 键继续承载 f3 手动折叠语义（重修后的显性
  文字钮写入）；滚动临时层依旧不落盘。
- **R-c 不做**：不做「入口恒展开」（推翻 f3 持久偏好语义——重修后的钮
  已显性，用户主动折叠的意图值得尊重）；不做迁移开关（一键清，无灰度
  必要）。

## Acceptance

- 存取层测试（headerCollapse.test.ts 新建）：v1 在场 → 读作展开且 v1
  键被清；v2 在场 → 正常判读且不碰 v1；读写往返回归；注入方缺
  removeItem 兼容臂。
- 渲染回归全绿（含 F4 否决臂钉子）；typecheck / i18n / theme /
  dead-css 绿。
- 真机复验：入口为展开态（旧用户），滚动触发自动折叠可见、文字钮可
  见可点。

## Resolution

已施工收口（2026-11-18，feat/learn1116）：键升版 v2 + 读侧 v1 一次性
清理（removeItem 可缺省）；headerCollapse.test.ts 新建 4 例（v1 作废
清理 / v2 唯一事实源 / 读写往返 / 缺 removeItem 兼容臂）；渲染回归
27 绿（含 F4 钉子）。真机复验待用户走查。
