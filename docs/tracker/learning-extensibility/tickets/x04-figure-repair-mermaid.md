---
id: x04
title: "figureRepair 支持 mermaid——失败的 mermaid 块有 AI 修复入口"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# x04: figureRepair 支持 mermaid

**What to build:** 把 mermaid 接进既有图修复链路，与 svg/jsxgraph 同等待遇。

## 现状（2026-11-01 实码核实）

- `ui/src/renderer/components/Markdown/figureRepair.ts:11` `language: 'svg' | 'jsxgraph'`
  封闭 union，无 mermaid。
- `MermaidBlock.tsx:100-118` 渲染失败 → 切源码视图，**无修复入口**；对照
  `SvgBlock.tsx:51-67`、`JsxGraphBlock.tsx:134-179` 出错即调 AI 修复。
- 后端 `RepairFigureRequest`（`nomifun-learning/src/models.rs:317`）与路由 `routes.rs:128`
  （实现会话复核）——确认 language 校验是否同样封闭。
- f3 已落 mermaid 机械 lint（括号配平/节点定义）；本票是**渲染期**兜底，不重复 lint。

## 实施要点

- [ ] 前端 union 加 `'mermaid'`；`MermaidBlock` 失败分支接 `figureRepair`（复用 Svg 块的
      修复 UI 形态）。
- [ ] 后端 language 白名单同步；修复提示词按语言族取（mermaid 的 parse error 随请求带上）。
- [ ] 单测：三种语言各一条修复请求形状；mermaid 失败→修复→重渲染闭环前端测试。

## 验收

- `bun run typecheck`；`cargo test -p nomifun-learning`；真机：故意写坏的 mermaid 节显示
  修复按钮并可修复。

## 非目标

- fence 注册表单源化（x17，本票是它的最小切片）。
