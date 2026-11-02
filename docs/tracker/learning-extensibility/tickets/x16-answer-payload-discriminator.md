---
id: x16
title: "答案 payload 自带判别——attempts.response 可回放、前端按 kind 分发"
type: task
status: open
assignee:
blocked_by: [x09]
part_of: ../map.md
---

# x16: 答案 payload 自带判别

**What to build:** 题目答案与作答对象自带 `kind`（版本由 x09 信封承担），`validate_shape` 与
前端渲染按判别分发而非按 `typeof` 猜形。

## 现状（盘点 2026-11-01，行号实现会话复核）

- `ActivityPack.answer: Value`（`models.rs:1437`）、`SubmitAttemptRequest.response: Value`
  （`:2614`）、`ReviewAnswerResult.correct_answer: Option<Value>`（`:2732`）——形状完全由
  `kind` 隐式决定，`validate_shape`（`:1524`）逐 kind 手工校验是唯一事实源。
- `learning_attempts.response_json`（**已发布** 015）无 kind 列，要 join activities 才知道题型；
  题型增减或答案形状调整后历史作答**无法回放**。
- 前端 `ReviewSession.tsx:82-93 answerText` duck-typing（x03 补漏洞，本票修结构）。

## 实施要点

- [ ] 答案对象：`{"kind":"numeric","value":3.14,"tol":0.01}` 等——`kind` 冗余自带；Rust 侧
      `#[serde(tag="kind")] enum Answer { SingleChoice{..}, Numeric{..}, … }` 替代 Value +
      validate_shape（validate_shape 退化为 enum 反序列化 + 语义校验）。
- [ ] `attempts` 加 `kind TEXT`（冗余，写入时取自 activity）；015 已发布 → **追加**
      `ALTER TABLE ADD COLUMN`，历史行 NULL。
- [ ] 读侧兼容：历史裸 answer（无 kind）按 activity.kind 补判别（x09 的 v0 路径）。
- [ ] 前端：`Answer` TS 判别联合，`answerText` 按 kind switch + default。
- [ ] 测试：9 种 kind 往返序列化；裸旧形读通；未知 kind 显式 Unknown。

## 验收

- `cargo test -p nomifun-learning -p nomifun-db`；`bun run typecheck`；
  `learning-smoke quiz` 档 EXIT=0。

## 非目标

- 新题型。
