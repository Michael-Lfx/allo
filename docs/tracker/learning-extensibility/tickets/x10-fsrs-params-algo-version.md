---
id: x10
title: "FSRS 参数算法版本包装——fsrs crate 升级不再静默误读用户权重"
type: task
status: closed
assignee:
blocked_by: [x21]
part_of: ../map.md
---

# x10: FSRS 参数算法版本包装

**What to build:** 用户自定义 FSRS 权重以 `{algo, w}` 包装落设置表；读侧未知算法回退库默认；
复习日志记算法标签。

## 现状（盘点 2026-11-01，行号实现会话复核）

- `nomifun-learning/src/scheduler.rs:20` `SchedulerSettings { desired_retention: f32,
  parameters: Vec<f32>, tz_offset_minutes }`；`service/checkin.rs:7-40` 从设置键
  `learning.desiredRetention`（clamp 0.7–0.99）/ `learning.fsrsParameters`（裸数组）读。
- `scheduler.rs:104-126` `predicted_retrievability` 取 `w[20]` 或 `FSRS6_DEFAULT_DECAY`
  ——**FSRS-6 权重布局假设硬编码**。fsrs crate 升到不同布局会静默误读用户参数。
- 记忆状态：`learning_review_items`（已发布 015/039）/ `custom_questions`（069）
  `stability_days/difficulty(CHECK 1..10)/review_count/lapse_count`；
  `learning_review_log`（**已发布** 052）逐次快照 S·D·r_pred——是可重放的优化训练集，
  但没记算法版本。

## 实施要点

- [ ] 设置值形状：`learning.fsrsParameters = {"algo":"fsrs6","w":[...]}`；读侧兼容裸数组
      视为 fsrs6；`algo` 不识别 → 忽略用户参数用库默认 + warn 日志 + 设置页显式提示
      「参数来自旧算法，已回退默认」。
- [ ] `w[20]` 等位置访问收口到按 `algo` 分支的单源 accessor。
- [ ] `review_log` 加 `algo TEXT`（nullable，历史 NULL=fsrs6）——052 已发布 → 在 **x21 基线**内
      `ALTER TABLE ADD COLUMN`。
- [ ] `difficulty CHECK 1..10` 是 FSRS 域烘进已发布表：x05 保留清单里，记档「算法换域需
      重建」；本票不动。
- [ ] 前端设置页若直接编辑数组（`LearningModelSelector` 一带，复核）同步新形状。

## 验收

- `cargo test -p nomifun-learning -p nomifun-db`；裸数组/新形状/未知 algo 三样读测；
  `learning-smoke xp` 档 EXIT=0。

## 非目标

- 升级 fsrs crate；参数优化器。

## Resolution

**已关（2026-11-02，本提交）**。

- **设置形状**：`learning.fsrsParameters` 新形 `{"algo":"fsrs6","w":[…]}`；读侧旧裸数组
  照认（视为 fsrs6）。`scheduler.rs` 新增 `FSRS_ALGO` / `FsrsParams` /
  `parse_fsrs_params` / `SchedulerSettings::decay_component`；未知 algo →
  `Unsupported`，checkin 整体回退库默认 + warn；`FSRS::new` 失败补 warn。
- **review_log**：基线 §12 `ALTER TABLE learning_review_log ADD COLUMN algo TEXT`；
  真实推进行写 `fsrs6`，合成种卡行写 NULL（历史行 NULL = fsrs6 口径）。
- **前端**：`configKeys` 联合类型（裸数组 / 信封）；设置页双读、未知 algo 输入置空 +
  双语提示（新 key `fsrsParametersUnsupportedAlgo`）、保存恒写信封；hint 补「算法标签
  自动附加」。
- **测试**：旧数组/新信封同解析、未知 algo 显式回退、非数整组作废、decay 缺省与
  w[20] 分支。
- **验收**：`cargo test -p nomifun-learning -p nomifun-db` 全绿；`learning-smoke xp`
  EXIT=0（含合成/真实 review_log 写法）。
- 偏差：无（`difficulty 1..10` FSRS 域 CHECK 保留，x05 原条款）。
