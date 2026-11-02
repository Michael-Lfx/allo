---
id: x17
title: "可视化 fence 注册表单源——visual 词表/校验/提示词/渲染/修复不再三角锁死"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# x17: 可视化 fence 注册表单源

**What to build:** 一张 Rust 单源注册表 `{visual_code, fence_langs, repairable, renderer_hint}`
驱动后端 visual 词表、`validate_body` 围栏白名单、lint、提示词装配、API 披露与前端渲染/修复
分发；`visual` 列存稳定码。

## 现状（盘点 2026-11-01，行号实现会话复核）

- 后端 `visual` 中文词表 `公式/函数图/示意图/流程图/图表/表格/无`（`models.rs:1208-1214`）
  → `:590-612` 映射到围栏语言族 → `validate_body`（`:489`）写死 `svg/jsxgraph/mermaid`
  （`:533-535, 642, 698, 752, 800-816`）→ f3 的 viz lint → 提示词可视化承诺条款。
- 前端 `components/Markdown/CodeBlock.tsx:135-166` 按 fence 语言分发（latex/mermaid/svg/
  jsxgraph，其余退化代码块）；`figureRepair.ts:11` union（x04 补 mermaid）；
  `JsxGraphBlock.tsx:126` `new Function` 执行模型 JS（安全面记档，不在本票）。
- `learning_lesson_sections.visual`（**已发布** 051）落中文；新增一种图（markmap/plotly）至少
  改后端 4 处 + 提示词 + 前端 3 文件，历史 `body_md` 无版本可判别（x19 可解）。

## 实施要点

- [ ] Rust 注册表 `visual_registry.rs`：`VisualKind { code, zh_label, fence_langs, repairable,
      … }` 静态表；`validate_body`/lint/提示词装配/按节规则全部查表。
- [ ] `visual` 列改存码（`formula|function_plot|schematic|flowchart|chart|table|none`）；
      列已发布 → **不迁移**，读侧中文→码规范化一版（节正文可再生，x19 懒升级时顺带重写）；
      DTO 披露码 + 前端 i18n 双语 label。
- [ ] 前端：渲染分发表由后端 `GET /api/learning/meta/visuals`（或构建期常量）驱动，
      `figureRepair.language` 改开放 string + 注册表查 `repairable`。
- [ ] x12 互钉脚本把注册表码集与前端表对拼。
- [ ] 提示词：visual 承诺条款从注册表生成，避免两份手写。

## 验收

- `cargo test -p nomifun-learning`；`learning-smoke content` 档 EXIT=0、viz 读数在场；
  `bun run typecheck && bun run check:i18n`；真机：新增一个仅注册表声明的测试 visual 码走通
  生成→校验→渲染→修复。

## 非目标

- 新渲染器实现；`new Function` 沙箱化（安全票另立）。
