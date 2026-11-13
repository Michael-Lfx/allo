---
id: d3
title: "移除知识结构「重新生成」入口（首次生成保留；后端端点不动）"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# d3: 移除知识结构重新生成入口

**What to build:** 用户裁决：知识结构不再支持重新生成（遗留问题太多）。摘要栏
的「重新生成」按钮在**结构已存在**时不再渲染；结构缺失/生成失败时「生成」入
口保留（首次生成的失败重试路径，砍掉会死锁）。后端
`POST /graph/structure/regenerate` 端点保留（撤销声明同口径：仅撤前端入口，
留纠错通道）；WS `structure_*` 事件面是首次生成共用面，**不动**。

## 法证证据（2026-11-10 实码核实）

- 按钮：`LearningGraphWorkspace.tsx:439-449`——label 按 `structure?.structure`
  有无在 `learningGraphStructureRegenerate`（重新生成）/ 生成 间切换；
  `regenerateStructure` 回调 :181-198（`setStructureRunning` + POST + 失败
  Modal）。
- 依赖面：结构生成中状态机（:145-179）由 WS `structure_started/completed/
  failed/cancelled` 驱动——首次生成（建课管线内）与 regenerate 共用，必须保
  留；`refetchStructure`（回合命令后刷新）与 regenerate 无关，保留。
- i18n：`learningGraphStructureRegenerate`（zh/en 同段）随入口删除（对称删）；
  `learningGraphStructureGenerate` / `learningGraphStructureFailed` 保留（首
  次生成用）。
- 提示词面引用自查：`learningGraphWindowStructureMissing` 文案提「用上方「生
  成结构」按钮生成后…」——生成入口保留，文案不必改。

## 实施要点

- [x] 按钮渲染条件改为「仅当 `structure !== null && !structure.structure`」（尚
      无结构）时渲染，label 恒为「生成」；`regenerateStructure` 回调保留（内部
      调 regenerate 端点——首次失败重试语义上也是 regenerate，端点保留故不改
      调用）；`learningGraphStructureRegenerate` key 双侧对称删（11607→11606）。
- [x] grep 确认：`learningGraphStructureRegenerate` 全仓归零；learning.json 内
      其余「重新生成」命中（genRetryFresh / regenerateQuestion / 重写对话框提
      示）属其它功能面，不属结构重生成，不动。

## 验收

- `bun run typecheck` 绿；`check:i18n` 绿；`git diff --stat` crates/ 为空（本
  票范围零后端改动，d2/d4 的 rust 改动在各自票面）。
- 真机目验（用户侧）：已有结构的课程摘要栏无任何重生成按钮；无结构课程仍可
  「生成」；首次生成失败后可重试。

## 非目标

- 后端 regenerate 端点与测试（保留）；结构挂载/修补（mount/repair 是维护回合
  不是重生成，不动）；c5 的结构自动呈现（依赖 structure_completed，不受影响）。

## Resolution（2026-11-10 施工收口）

**代码面全验收过；真机目验待用户。** 施工 = `c2c7371d5`。

- 摘要栏按钮收敛为「仅无结构时渲染的『生成』」；回调与 WS 状态机原样（首次
  生成失败重试仍可用，走保留的 regenerate 端点）。key 双侧对称删除
  （11607→11606，`check:i18n` 绿，gen 后 `learningGraphStructureRegenerate`
  在 i18n-keys.d.ts 归零）。
- crates/ 本票零改动（`git diff --stat` 实证）。
