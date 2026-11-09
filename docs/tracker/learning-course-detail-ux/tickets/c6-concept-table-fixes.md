---
id: c6
title: "概念表修复：渲染 definition / 互指覆盖写 bug / rowKey / 审计失败降级不整表报错"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# c6: 概念表修复

**What to build:** 课程内概念表让用户看得到「这个概念到底是什么意思」：概念列
在名称下渲染 `definition`（两行 ellipsis + Tooltip 全文）。顺带修三处缺陷：
互指只保留最后一个 partner 的覆盖写 bug、表无 `rowKey`、概念列表与审计共享
一个 error state 导致审计失败打翻整表。

## 法证证据（2026-11-09 实码核实）

- 定义未渲染：`GraphConceptRowView.definition`（types.ts:909-924）每行已由
  `GET graph/concepts` 返回，`ConceptTable`（LearningGraphWorkspace.tsx:849-1052）
  概念列（:965-992）只渲染 canonical + 别名行（含别名定义 :985），**概念自身
  定义从不出现**（截图二：用户无法判断概念是不是那个意思）。
- 互指覆盖写：:897-900 `mutualWith.set(a, b)` / `set(b, a)`——一个概念参与多
  组互指时只剩最后一个 partner。
- 无 rowKey：`<Table>`（:958-964）未设 `rowKey`。
- 共享 error：graphConcepts（:865）与 conceptAudit（:875）失败写同一个
  `error`，:905 有 error 即整表替换为错误；`concept_audit` 对 archived 课程
  返回 409（learning_graph.rs:1619-1622），`graph_concepts` 无此守卫——审计
  失败不应吞掉可用的概念列表。

## 实施要点

- [ ] 概念列：canonical 行下加定义副行（`Typography.Paragraph` /
      `Text` ellipsis 2 行，Tooltip 全文）；空定义不渲染占位。
- [ ] 互指：`Map<string, string[]>` 收集全部 partner；审计列逐个渲染
      「互指·{name}」chip。
- [ ] `rowKey='concept_id'`。
- [ ] error 拆分：列表失败 = 整表错误态；审计失败 = 表照常渲染、审计列降级
      （一行次要提示「审计不可用」，新 i18n key 双侧对称）。

## 验收

- `bun run typecheck` + `check:i18n` 绿。
- 目验：概念表每行可见定义；含多组互指的概念全部 partner 都显示；人为让审计
  失败（archived 课程或断网模拟）概念列表仍在。

## 非目标

- 行点击进详情、页头「全部概念」入口、other_courses 可点链接（c8，依赖 c7
  端点与 DTO）；后端改动（c7）。
