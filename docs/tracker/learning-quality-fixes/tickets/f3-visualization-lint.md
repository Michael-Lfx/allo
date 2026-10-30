---
id: f3
title: "课时可视化块机械 lint——坏 mermaid 不再过闸"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# f3: 课时可视化块机械 lint

**What to build:** 课时正文的 ```mermaid 可视化块加最小机械校验（围栏内语法
配平检查）——坏图打回重写一次，仍败降级剔除 + audit findings 留痕，不留坏
图进库。

## 法证证据（e2e2 真课「滚雪球」s1 节）

正文数学全对（110/121/133.1 ✓），但概念节的 mermaid 块：
- **语法坏**：`A[["第1期 本金 P] -->|"算利息 P×利率"| B[利息1"]]`——节点 A
  `[[` 开、单 `]` 收（括号不平衡），B 内游离引号；
- **语义碎裂**：A→B 一段 + C→D、E→F→G 两段，**三块互不连通**，不是一张图
  而是三个碎片。
- 成因画像：VISUAL_STANDARD 门要求「≥1 可视化块，纯文字会被质检门拒绝」——
  模型为过门交了低质图形（卡门产物）。对照：另一课同门的 ```svg 块质量良好
  （svg 有 sanitize 先例、生成更稳）。

## allo 现状（落点提示）

- 可视化门条款：`generation/lesson.rs:747-749` VISUAL_STANDARD_BLOCK（传统
  线）+ `nomifun-ai-agent/src/lesson_content_loop.rs:90`（图线，$$公式$$/
  ```svg/```jsxgraph/```mermaid/表格 五选一）。
- 装配/校验面：课时草稿 op 流（ls_set_section_body）与审计 findings
  （lesson_draft.rs 的 audit 观察条先例：over_capacity/naming_*）——lint
  落点应在草稿 op 接纳处或装配校验处（实现会话定位，两面取一，别双处）。

## learnhub 参考

- `engine/contentgen/validator.rs`（cc.* 条款族）：**他们没有可视化 lint**
  （cc.steps.present 只是节数下限）——本票是 allo 首创小闸；可参考的只有
  其「条款打回→重写→仍败降级」的通用形状与 svg sanitize 先例
  （`ui/src/renderer/components/Markdown` 的 CodeBlock svg 路由是消费侧）。
- learnhub 分面裁决（ADR-0014）注记：正文走文本接缝避开 JSON 转义雷区——
  同理 mermaid 语法错误模型侧自检不可靠，**机械闸是唯一可靠的**。

## 实施要点

- [ ] **最小语法闸**（Rust 侧轻实现，勿引 mermaid 解析器依赖）：围栏内逐行
      检查——①方括号/圆括号配平（`[[`→`]]` 成对）；②节点 id 被边引用则须
      有定义；③`---` 分隔符后页面声明的闭合。只做「明显是坏的」三查，
      不追求全语法。
- [ ] **打回一次 + 降级**：lint 失败 → 打回该节重写（带死因「mermaid 语法
      破损：括号不平衡」指引）恰一次；仍败 → **剔除该可视化块**（节正文
      其余保留）+ audit findings 留痕（新观察条 `viz_block_lint` warning，
      带节 key 与原因）——正文不因图烂而整课失败。
- [ ] 两种可视化形态同闸：```mermaid 全检；```svg 已有 sanitize（消费侧）
      ——生成侧只补「非空且含 <svg 标签」最小检查（防空壳块过门）。
- [ ] 红绿样：坏括号 mermaid 被打回；重写成功过闸；两连败降级剔除 + findings
      行；svg 空壳块打回。
- [ ] smoke content 读数行补 `viz_lint_events`（读数非门禁）。

## 验收

- `cargo test -p nomifun-ai-agent` + `-p nomifun-learning` 全绿；fmt；零迁移。
- 真机判据：连生两课，可视化块 lint 零打回或打回后自愈；audit findings 无
  viz_block_lint warning（出现即如实记录）。

## 非目标

- mermaid 全语法解析器（引依赖，超预算）。
- 可视化门本身（≥1 块的要求不动；模型改交表格/公式也合法）。
- jsxgraph 形态（先例少，随 mermaid 闸的括号检查顺带覆盖，不单列）。
