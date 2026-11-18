---
id: pf5
title: 题型注册表与忽略机制（descriptor 单源 + 五消费面派生）
type: task
status: open
assignee:
blocked_by: []
blocked_note: pf1/pf2/pf3/pf4 的题型能力消费全部挂靠本票 descriptor，施工最前
part_of: map.md
---

## Question

题型会反复发生废弃/新增/变更（matching 废弃是第一例）。一次性废弃清单
只解决单例——需要**通用系统**：每题型一条 descriptor 的注册表，全部消
费面从表派生，未来变更 = 改一条 descriptor 的单点操作。

## 已核验事实（2026-11-18 影响面全扫）

**机制形态（非取舍裁定）**：代码内注册表单源（TS descriptor + Rust
capability 方法，语义对齐靠同源测试）；不做运行时配置（题型语义是代码
审阅级事实）。两态 `active | deprecated`（+ `replacedBy`），不预留多态。

**descriptor 字段**：`status` / `replacedBy` / `llmGeneratable`（进生成
契约）/ `countsTowardSection`（计节完成门槛）/ `reviewEligible`（进复习
抽题）/ `perItemFeedback`（pf1 逐项反馈能力）。

**首例数据**：matching = `deprecated, replacedBy: classify,
llmGeneratable: false, countsTowardSection: false, reviewEligible: false,
perItemFeedback: true`（存量兼容渲染面保留）；其余八 kind = active 全
能力（reflection/open_question perItemFeedback=false——AI 批改语义）。

**影响面全清单（逐点定性，grep 全扫）**：

| 消费点 | 位置 | 注册表化动作 | 风险 |
| --- | --- | --- | --- |
| 节完成门槛 | model.ts:51-102 `stepActivities`/`stepLocked` | round 过滤 `countsTowardSection=false` 行；既有 `round.length>0` 守卫**天然实现**「全 deprecated 节自动通过」（Q3 裁决零成本落地） | 低 |
| 复习抽题 | review.rs draw 查询（:795-815 邻域 join activities） | draw 排除 `reviewEligible=false` kind | 低 |
| 生成契约 | lesson_content_loop.rs:226/:1612 手写 prose + PATCH schema 枚举 | prose 手工移除 matching（pf2 R-a2）+ **一致性测试**断言 prose/schema 集合 == registry `llmGeneratable` 集合（LLM 面文本是手调的：取一致性测试不取机械派生） | 中（prose 变更=LLM 面行为变更，smoke 兜底） |
| 活动审计 | generation/activities.rs + lesson_draft.rs 校验面 | ①classify 新 kind 的 config 校验（categories distinct 机械断言，pf2）；②`status=deprecated` 行发 warning finding（不挡发布） | 低 |
| 判分 | progress.rs evaluate 各臂 | **Matching 臂保留**（存量兼容判分）；classify 臂新增（pf2）；pf1 逐项结果按 `perItemFeedback` 挂靠 | 低 |
| 练习渲染 | ActivityInput.tsx 分支 + MatchingInput | **分支保留**（存量兼容渲染）；练习流不出现（round 过滤的下游结果）；l1 布局修复随之自然不触发 | 零 |
| 复习卡渲染 | reviewAnswerText.ts + review.rs 卡片映射（:795-815） | kind 映射保留；draw 排除后实际不再出现 | 零 |
| 教程种子 | tutorial.rs | **全文零 matching**（grep 计数 0，:315 match 臂仅为穷尽性覆盖）——无冲突 | 零 |
| quiz 面 | 题库独立管道 | 不受影响 | 零 |
| XP 结算 | settle.rs | kind-agnostic，不受影响 | 零 |
| AI 批改 | open_judge.rs | reflection/open 专用，不受影响 | 零 |
| **复习卡种子** | progress.rs `seed_lesson_review_items` | **跳过 deprecated kind 活动（不种卡）**——2026-11-18 兼容性复核补入：完成旧课时会给永不抽出的 matching 活动种卡（due 徽标虚高 + 死卡堆积）。存量实测：matching 活动 4 道、关联复习卡 **0 张**（当前无存量死卡，但完成旧课时即触发种子路径） | 低 |

**账本零触碰**：已作答 matching attempt/XP/打卡原样保留（学习账本独立
性）。

## Terms（施工定案，2026-11-18 grilling 收口）

- **Q1=A 消费面全套**（上表 1-5 全做）；**Q2=A 存量练习流隐藏**（不渲
  染 deprecated 活动）；**Q3=A 全 deprecated 节自动视为完成**（过滤后
  既有守卫天然落地）。
- 注册表落点：TS = constants 或独立 `activityKindRegistry.ts`（per-kind
  descriptor 表）；Rust = ActivityKind 关联 capability 方法或常量表。
  两端语义对齐靠同源测试。
- pf1/pf2/pf3/pf4 挂靠点全部改为 descriptor 派生（pf1 perItemFeedback /
  pf2 llmGeneratable+审计 / pf4 题型指导按判别/联结轴分组）。
- **⑥复习卡种子面（2026-11-18 兼容性复核补入）**：
  `seed_lesson_review_items` 种卡跳过 `reviewEligible=false` 的活动——
  完成旧课时不再给永不抽出的 matching 活动种卡（否则 due 徽标虚高 +
  死卡堆积）。存量实测：matching 活动共 4 道、关联复习卡 **0 张**（当
  前零死卡，本条防未来完成旧课时触发种子路径）。+ 测试（旧课时完成 →
  matching 零种卡、非 deprecated 正常种卡）。

## Acceptance

- 注册表单测：九 kind descriptor 全量覆盖；deprecated 派生行为四分支
  （门槛/抽题/契约一致性/审计 finding）。
- stepActivities 过滤测试：matching 行不计门槛、全 deprecated 节自动过、
  非 deprecated 正常。
- 复习抽题排除测试；契约一致性测试（prose/schema == registry）。
- 复习卡种子跳过测试（旧课时完成 → matching 零种卡、非 deprecated
  正常种卡）。
- `cargo test -p nomifun-learning` / typecheck / 聚焦前端测试 /
  theme / dead-css 绿；smoke EXIT 0（prose 变更兜底）。
- 真机：练习流不再出现该匹配题、节可直接过、复习不抽到（随波欠账）。

## Resolution

（未结案）
