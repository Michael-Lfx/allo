---
id: x03
title: "复习答案渲染 number 类型——numeric 题正确答案显示为空串"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# x03: 复习答案渲染 number 类型

**What to build:** `ReviewSession` 揭示正确答案时按答案形状全集渲染，number（numeric 题）
与对象形答案不再静默变成空串。

## 现状（2026-11-01 实码核实）

- `ui/src/renderer/pages/learning/components/ReviewSession.tsx:82-93` `answerText`：
  boolean → 真/假 i18n；数组 → 仅保留 string 元素；其余
  `typeof value === 'string' ? value : ''` → **number 落空串**；matching 等对象形同样落空。
- 调用点 `:300` `<InlineMarkdown text={answerText(result.correct_answer)} />`。
- 答案形状由题型隐式决定（`models.rs:1437 answer: Value`，`:1524 validate_shape`），前端
  只能 duck-typing——这是 x16 的根因，本票只补渲染漏洞。

## 实施要点

- [ ] `answerText` 补 number（`String(value)`；tol 披露由后端 feedback 承担）、数组内
      number、对象形（matching：`k → v` 逐对；ordering：按序号列出）。
- [ ] 未知形状兜底：`JSON.stringify` 可见降级，不吞空。
- [ ] 单测：对 9 种 `ActivityKind` 的 `correct_answer` 样例各断言非空字符串。

## 验收

- `bun run typecheck`；前端单测绿；真机：numeric 自建题答错 → 揭示区显示数值。

## 非目标

- 答案 payload 加判别字段（x16）。
