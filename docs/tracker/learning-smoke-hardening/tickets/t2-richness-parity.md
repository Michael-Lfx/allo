---
id: t2
title: "产出丰富度追平 learnhub——正文篇幅 / 题目 evidence / 铸名出处"
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

2026-10-16 产出质量对账（对照 learnhub `ldata1` 数学课 + llm-calls 档案）裁
定「大致持平、验收线达标」，但有三处**丰富度**差距悬而未决。本票裁决追哪些、
追到什么口径：

1. **正文篇幅**：allo 单篇 5.5KB/4 小节（概念/演示/例题/练习，SVG+KaTeX）
   ≈ learnhub 均值 13.6KB（9.3–27KB）的四成。部分是节点粒度差（allo 更
   细），但同粒度下 learnhub 讲得更展开。追法：content 档提示词补篇幅纪律
   条款（各小节的展开度下限），复测读数（字数/小节）对照 learnhub 分位。
2. **题目 evidence 与逐选项辨析**：learnhub 题库每题带 `evidence`（题干证据
   回引）+ explanation 逐选项说明错因；allo 解析只覆盖正确答案。追法：出题
   提示词补两条款 + `learning_activities.config_json` 契约扩展（evidence
   列），读侧（判底展示）随行。
3. **概念铸名 source 出处**：learnhub 铸名带教科书引用（`source: 人教版
   《义务教育教科书·数学》七年级上册 · 第一章 有理数`）；allo registry 无
   出处字段。追法：铸名提示词补 source 条款 + registry 小迁移（source 列，
   可空）——learnhub 口径「出处是簿记不是判定」，不进门禁。

裁决要点：三项独立可拆；验收 = 复测读数对照 learnhub 分位（不是逐字节对
齐）；模型固定 flash（用户明令，不换模型）。

## Resolution

（结案时追加）
