---
id: c3
title: "删撤销声明（仅前端全链；后端 DELETE /basis 保留）"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# c3: 删撤销声明（仅前端）

**What to build:** 删除撤销声明的整条前端链：GateCoverage 的撤销按钮与
`confirmRevoke` 确认弹窗、`revoking` / `onRevoke` props，LearningGraphWorkspace
的 `revokeBasis` 回调与 `revokingBasis` state，DeclarationWindow /
StructureGraphTab 的透传 props，`api.ts` 的 `revokeBasis` client 方法与孤儿
类型，以及 `learningGraphBasisRevoke*` 系列 i18n keys（zh-CN/en-US 双侧对称
删除）。后端 `DELETE /basis` 路由、服务与测试**原样保留**（用户裁决 1：幂等
且测试锁定，留作纠错通道）。

## 法证证据（2026-11-09 实码核实）

- 前端链：`GateCoverage.tsx:71-82`（confirmRevoke）、:167-171（撤销按钮，
  `basis.declarations > 0 && onRevoke` 时显示）、:21/:29（revoking/onRevoke
  props）；`LearningGraphWorkspace.tsx:246/:305-319`（revokingBasis state +
  revokeBasis 回调）；透传 `StructureGraphTab.tsx:40-41/:51-52/:161` 与
  `DeclarationWindow.tsx`（同形 props 面）。
- client：`api.ts:175-180` `revokeBasis`（DELETE
  `/api/learning/courses/{id}/basis`，返回 `{ revoked: number }`）。
- i18n keys（zh-CN/en-US `learning.json` 同段）：`learningGraphBasisRevoke`、
  `learningGraphBasisRevokeConfirmTitle`、`learningGraphBasisRevokeConfirmContent`、
  `learningGraphBasisRevokeConfirmOk`、`learningGraphBasisRevokeFailed`。
  `learningGraphBasisCancel` 被声明确认弹窗共用，**保留**。
- 后端不动面：`routes.rs:91-97` 三条 basis 路由、`revoke_basis`
  （learning_graph.rs:5628-5667）、`basis_revoke_deletes_anchor_keeps_concepts_and_source_mark`
  等测试全保留。
- 撤销语义（留档）：全删锚行 all-or-nothing，概念留池
  （source='basis-declaration' 标记不灭）；已知投影随读侧重算立即收缩——
  用户裁决其易产生不可预期错误，从产品面移除。

## 实施要点

- [ ] GateCoverage：删 confirmRevoke、撤销按钮、`revoking` / `onRevoke`
      props；`learningGraphBasisRevoke*` 引用清零。
- [ ] LearningGraphWorkspace：删 revokeBasis / revokingBasis；两个挂载点
      props 面同步收缩。
- [ ] api.ts：删 `revokeBasis`；`{ revoked: number }` 相关孤儿类型一并删
      （types.ts 如有）。
- [ ] i18n：五个 `learningGraphBasisRevoke*` keys 双侧对称删除；
      `bun run gen:i18n` → `check:i18n` 过。
- [ ] 全仓 `grep -rn "evokeBasis\|learningGraphBasisRevoke" ui/src` 归零
      （c1/c2 并行改动时以本票合并后状态为准）。

## 验收

- `bun run typecheck` + `check:i18n` 绿；grep 归零。
- 后端零改动可证：`git diff --stat crates/` 为空（本票范围）。

## 非目标

- 后端 DELETE /basis 与其测试（明确保留）；声明冻结（c1）。
