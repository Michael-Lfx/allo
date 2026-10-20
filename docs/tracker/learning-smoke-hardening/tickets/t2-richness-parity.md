---
id: t2
title: "产出丰富度追平 learnhub——正文篇幅 / 题目 evidence / 铸名出处"
type: task
status: closed
assignee: zhangyj
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

**速览**：条款 1、2 落地（commits `f0f1a07cc` + evidence 硬门增补）；条款
3 裁决缓行（迁移属高风险区，见下）。读数复测达标。

- **条款 1（正文篇幅）**：节正文提示词的篇幅纪律重写为「THE BUDGET IS
  THE FLOOR OF A FULLY-WORKED SECTION, NOT A CEILING」——档位预算
  （低 150/中 250/高 400 字）是展开度下限锚，落点 100%–150%，展开方式
  = 把节任务点名的每个要点教透（概念节配 worked mini-example、例题节算
  全程、演示节逐步注解），禁止复述式注水；>2000 硬拦与质检门下限不动。
  **复测读数**（flash 真机 content 档）：正文 3218 字/节 5——旧基线约
  1000–1400 纯文字的 2–3 倍，单篇全文档折算回到 learnhub 均值
  （13.6KB）分位。
- **条款 2（evidence + 逐选项辨析）**：`ActivityPack`/`StoredActivityConfig`
  增 `evidence`（≤120 字逐字回引，serde default 容旧行）；两个出题提示词
  （LESSON_SYSTEM / LESSON_ACTIVITY_SYSTEM）补形状与规则；choice 题解析
  必须「walk EVERY option」逐选项点名。落库三处（生成/导入/变式）透传，
  手工编辑保留原值；判底读侧 = `QuizGradeFeedback.evidence` 随 grade 下发
  （开考投影仍零判底，单测钉住）——前端展示随 quiz UI（g4 载体）落地。
  **实施中发现 flash 对「可选」条款遵循率 0/4（真机 content 复测）**，升
  级为主生成面硬门（缺/空 evidence 拒绝重试，`evidence_missing` +
  validate_lesson_activities 执法；lesson_draft 审计面加 `evidence_missing`
  warn 兜历史行）；变式/无正文语境容缺（判分底仍可携）。
- **条款 3（铸名 source）缓行裁决**：registry source 列是 append-only 迁
  移，AGENTS.md 高风险区要求先询 owner；且 learnhub 口径「出处是簿记不是
  判定」——不进门禁，缓行无门禁损失。建议随产品波与 t2 丰富度复测一起
  裁（届时 mint 的 source 语义对齐 new_aliases 的 attestation 口径：
  「教练提名：…」而非书目引用）。
- **验证**：`cargo test -p nomifun-learning` 全绿（提示词面断言 + evidence
  硬门正反样 + 脚本金样补 evidence）；content 档真机 EXIT=0。
