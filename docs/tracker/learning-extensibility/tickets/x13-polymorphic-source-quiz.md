---
id: x13
title: "多态 source 扩 quiz——复习日志/标签/打卡接入第三来源"
type: task
status: open
assignee:
blocked_by: [x05]
part_of: ../map.md
---

# x13: 多态 source 扩 quiz

**What to build:** `(source, item_id)` 多态引用从 `course|custom` 扩到 `quiz`，使 quiz 回合
的作答进入 review_log / review_events / question_tags 的统一口径。

## 现状（盘点 2026-11-01，行号实现会话复核）

- 三表同款多态（**均已发布**）：`learning_review_events.source`（042，`item_id` 语义随
  source 变）、`learning_review_log.source`（052）、`learning_question_tags.source`（036 +
  UNIQUE `(question_id, source, tag_id)`）；`ReviewSource` enum `models.rs:2666` course/custom。
- `learning_quiz_rounds`（084）已是第三种作答来源，但进不了复习日志/打卡/标签；
  `xp_events.anchor` 已有 `quiz:` 前缀（`service/quiz.rs:510`）说明结算侧已承认它。
- x05 去掉三表 source 列 CHECK 后本票才能不再重建表。

## 实施要点

- [ ] `ReviewSource::Quiz`；`item_id` 语义文档化为表驱动映射 `source → (目标表, 列)`，落
      `id_schema_contract.rs` 逻辑引用登记（course→review_items / custom→custom_questions /
      quiz→quiz_rounds 或 plan item 子键——开工定 quiz 的 item 粒度）。
- [ ] 写入点：quiz 结算路径补 review_log/review_events 行（FSRS 推进与否按到期门口径，
      见 `CONTEXT.md` 复习口径）；标签面是否开放给 quiz 题（产品面，可后置）。
- [ ] 读侧：复习统计/记忆健康面板按 source 聚合处不漏 quiz（grep `source =` 字面量）。
- [ ] 测试：三表各插 quiz 行 + 聚合读数包含。

## 验收

- `cargo test -p nomifun-learning`；`learning-smoke quiz` 档 EXIT=0 且 review_log 出现
  `source=quiz` 行读数。

## 非目标

- quiz 产品形态改动。
