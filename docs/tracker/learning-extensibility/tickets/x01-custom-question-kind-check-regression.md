---
id: x01
title: "自建题 kind CHECK 回退修复——069 重建把已发布的 9 种题型收窄回 3 种，升级可中断"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# x01: 自建题 kind CHECK 回退修复

**What to build:** 让 `learning_custom_questions.kind` 的 DB 约束与**已发布词表**和代码放行
词表一致。当前：① 用户库里只要有一行 4 类自建题，升级迁移中断 → 应用启动失败；
② 新建 multi_choice / numeric / ordering / matching 自建题撞 CHECK → 500。

## 现状（2026-11-01 实码核实）

- **已发布** `050_learning_sections_and_question_kinds.sql:99-101`（v1.6.x/v1.7.0 用户库）
  把该表 kind 扩到 9 种（`ActivityKind` 全集）。
- 本分支未发布的 `069_retire_graph_dag_and_concepts.sql:118-122` 重建表时
  `kind IN ('single_choice','true_false','fill_in_blank')`，注释自述「以 038 的宽口径为准
  （三种题型）」——漏看 050；且**没有任何 DELETE/UPDATE 处理存量 4 类行**（grep 无
  multi_choice/numeric/ordering/matching）。INSERT…SELECT 搬行时撞 CHECK → 迁移失败 →
  checksum 锁下用户无法启动。
- 代码 `crates/backend/nomifun-learning/src/service/review.rs:817-826`
  `create_custom_question` 放行 7 种（拒绝 AI 判分的 reflection/open_question）。
- 前端自建题创建入口只暴露 3 种（`pages/learning/components/QuestionDialogs.tsx:365-367`），
  所以 UI 路径的存量库大概率干净；但 HTTP API 与 agent 工具放行 7 种，不能赌。
- 迁移测试 `crates/backend/nomifun-db/tests/learning_concept_web_migration.rs:273`
  把三种口径钉成了「正确」。

## 为什么必修

升级中断是最高等级事故；且 069 自己的注释就写着「收窄会让 038 之后落库的自建填空题在新表上
必然 CHECK 失败」——它对 050 做了同样的事。

## 实施要点

- [ ] 069 未发布 → **原地改**：CHECK 词表取 `ActivityKind` 全集 9 种（与已发布的 050 同集，
      **永不收窄已发布词表**）。不要只补到代码放行的 7 种。
      若 x05 先落地（去 CHECK），本票收口为「仅修测试 + 补回归样」。
- [ ] 修 `learning_concept_web_migration.rs` 断言：**fixture 先按 050 形状插入 9 种 kind 各
      一行**，再跑 069 至尖端，9 行全部存活。这是 map 裁决点 1「已发布表 fixture 基线」的
      第一个样板。
- [ ] 加 Rust 单测互钉：`ActivityKind` 全变体 `as_str()` 都能 INSERT 进
      custom_questions（内存库跑全链）。这是 x12 互钉测试的第一块。

## 验收

- `cargo test -p nomifun-db` + `cargo test -p nomifun-learning` 全绿。
- HTTP：`POST /api/learning/custom-questions` kind=multi_choice/numeric/ordering/matching
  各建一题 → 201 并出现在复习队列。
- 用 v1.7.0 形状 dev 库（含 4 类自建题行）启动尖端 → 迁移通过。

## 非目标

- 其它词表列的 CHECK 治理（x05）。
- 前端自建题菜单只开放 3 种——产品面，map 裁决点 5 / x12。
