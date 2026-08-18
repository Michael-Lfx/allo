# 学习模块改进：凌晨 2 点到期统一 + 新卡次日首复 + 每日打卡（2026-08-18）

> 定稿日期：2026-08-18。范围是 `nomifun-learning` 的复习调度（`crates/backend/nomifun-learning/src/scheduler.rs`、`service.rs`）与前端「今日复习」/「问题管理」/学习设置页。本文只描述规则与契约，代码落点见第 7 节；本文定稿后按第 9 节逐项实施，任何规则的调整必须先改本文。

## 1. 结论与强制规则

复习调度从"相对复习时刻的精确偏移"改为"复习日 + 固定到期时刻"，并新增两项配套规则：

1. **凌晨 2 点为复习日日界线**。本地时区 `02:00` 视为新一天的开始；`00:00–01:59` 的复习与入库都归属于"前一天"（2 点视为第二天，适配熬夜用户）。任何 day-level（间隔 ≥ 1 天）的排期，`due_at = 到期日 02:00`。
2. **新卡首复排期到次日**。新入库的卡片（课时完成入队、手动添加问题）不再 `due_at = now` 立即到期，而是 `due_at = 复习日(now) + 1 复习日的 02:00`，即"明天到期"。
3. **每日打卡完成即锁定**。用户设置每日目标 N；当日累计复习 ≥ N、或判定时刻无到期卡，任一满足即打卡完成并锁定当天状态；锁定后当天新增的到期卡不改变打卡状态，作为"额外任务"继续出现在队列。

强制细节：

- `sub-day` 重学步骤（`Again`/`Hard` 后间隔 < 1 天）**保持精确时刻**，不抹平到 02:00；跨过 02:00 的重学卡自然滚动到新复习日。
- FSRS 输入 `days_elapsed` 改为**复习日差**（`review_day(now) - review_day(last_reviewed_at)`），不再用精确时间差整除。
- 队列判定语义不变：`due_at <= now` 即为到期（今日复习、问题管理、角标计数三处共用）。
- 时区由前端上报（`learning.tzOffsetMinutes`），未上报时回退服务器本地时区；后端所有计算统一走该时区。
- 存量 `due_at` 不做数据迁移，自然过渡：存量卡到期并完成下一次复习后自动进入新规则。

## 2. 现状与问题

### 2.1 现状（代码事实）

`schedule_review`（`scheduler.rs`）当前规则：

- 间隔 ≥ 1 天：`due_at = 复习时刻 + round(interval) × 24h`——到期时刻永远跟随复习时刻漂移（晚上刷的永远晚上到期）。
- 间隔 < 1 天：`due_at = 复习时刻 + interval`，下限 `MIN_RELEARN_DELAY_MS = 1 分钟`。
- `days_elapsed = (now - last_reviewed_at) / 24h` 取整后喂给 FSRS。
- 全部使用 UTC 毫秒（`TimestampMs`），无任何时区概念。
- 新卡入队两处均为 `due_at = now` 立即到期：课时完成入队（`service.rs` 约 4155–4164 行）、手动添加问题（`service.rs` 约 2407–2422 行）。
- 用户偏好存 `client_preferences`（现有 key：`learning.desiredRetention`、`learning.fsrsParameters`，见 `service.rs` 的 `scheduler_settings()`）。

### 2.2 三类问题

| 问题 | 表现 | 根因 |
|---|---|---|
| 到期时刻漂移 | 「今日复习」边界模糊：白天打开只有少量卡，晚上又冒出一批 | day-level 排期相对"复习时刻"偏移，而非相对"日历日" |
| 新卡当天重复 | 刚读完课文和练习题，客观题立即出现在复习队列 | 入队时 `due_at = now` |
| 打卡目标不稳定 | 当日到期数随重学卡、新题不断增长，目标永远"差一点" | 无打卡锁定机制 |

## 3. 复习日与凌晨 2 点到期统一

### 3.1 复习日定义

- 设时区偏移为 `tz`（分钟，UTC 向东为正）。某时刻 `t` 的复习日为：`t` 换算到本地时间后，若本地时刻 < `02:00` 则为前一天，否则为当天。即日界线固定为本地 `02:00`。
- 用 `review_day(t)` 表示 `t` 所属复习日（一个整数日期值，实现为本地日期序数或 UTC 毫秒归一化值均可，全库统一即可）。
- 语义示例（本地时区）：

| 本地时刻 | 所属复习日 | 说明 |
|---|---|---|
| 08-17 01:30 | 08-16 | 熬夜未睡，属于 08-16 复习日 |
| 08-17 02:00 | 08-17 | 新的一天开始 |
| 08-17 23:59 | 08-17 | 当天最后一刻 |

### 3.2 day-level 调度（间隔 ≥ 1 天）

```
到期日 = review_day(now) + round(interval) 天
due_at = 到期日 02:00（本地时间换算为 UTC 毫秒）
```

- `interval` 取整规则沿用现有 `round()`。
- `due_at` 必须落在到期日 `02:00`，与复习时刻无关——同一天内任何时刻复习同一张卡，下一次到期时刻完全相同。
- 实际间隔范围：`[22h, 48h)`（凌晨 03:00 复习的 1 天卡实际 23h 到期，白天复习的 1 天卡实际 22–24h 到期），在 FSRS interval 本身 ±12h 取整噪声之内，不为此增加"至少 24 小时"下限。

### 3.3 sub-day 重学步骤（间隔 < 1 天）

- **保持精确时刻**：`due_at = now + interval × 24h`，下限沿用 1 分钟。遗忘后 10 分钟~几小时内的重学是间隔重复的核心机制（在遗忘曲线早期强化），抹平到次日 02:00 会让中午遗忘的卡最长等待 14 小时，学习效果显著变差。
- 跨日滚动自然成立：`23:50` 遗忘、30 分钟后 `00:20` 到期（仍在原复习日，熬夜用户可继续刷）；`01:40` 遗忘、30 分钟后 `02:10` 到期（已进入新复习日）。
- 不做"深夜顺延"（如 22:00–02:00 区间顺延到次日）：阈值拍脑袋且与熬夜用户的实际作息冲突。

### 3.4 FSRS 输入：复习日差

- 现状：`days_elapsed = (now - last_reviewed_at) / 24h` 整除，存在 ±1 天噪声（例如 23:50 与 00:10 复习同张卡，时间差 20 分钟，整除后都是 0 天，正确；但 23:50 与次日 23:40 相差 23h50m，整除为 0 天，实际已隔一天）。
- 改为：`days_elapsed = review_day(now) - review_day(last_reviewed_at)`。
- 效果：抹平后 FSRS 输入反而是**更准**的复习日差；首次复习（`last_reviewed_at = None`）仍为 0，与现状一致。

### 3.5 时区方案

- 前端在启动时读取 `Date().getTimezoneOffset()`（分钟），写入 `client_preferences`，key：`learning.tzOffsetMinutes`。
- 后端读取该值参与所有复习日计算；未上报或缺失时回退服务器本地时区（`chrono::Local` 或系统 `utc_offset`，实现时取后端进程时区）。
- 时区变化（用户旅行、改系统设置）会导致存量 `due_at` 相对本地时间的语义整体平移，属可接受行为（Anki 同款）；下次复习后自动收敛到新时区。
- 时区偏好变化**不需要**迁移存量数据。

### 3.6 队列语义与存量数据

- 全部队列查询保持 `due_at <= now` 不变（`service.rs`：今日复习列表、自定义题列表、今日到期计数、问题管理 `due`/`overdue` 判定）。
- 存量卡不迁移：旧规则排出的 `due_at` 到期后照常出现，完成下一次复习后由新规则接管。不做一次性归一化——归一化需要区分 day-level/sub-day（`due_at` 与 `last_reviewed_at` 的间隔阈值）且收益小（旧卡到期时刻与新规则的差异仅存在于首次到期）。

## 4. 新卡首复排期到次日

### 4.1 规则

新卡入队的两处入口统一改为：

```
due_at = review_day(now) 的下一个复习日的 02:00
      = (review_day(now) + 1 天) 的 02:00
```

- 课时完成 → 客观题入队（`service.rs` `INSERT INTO learning_review_items`，当前 `.bind(now)`）。
- 手动添加问题（`service.rs` `INSERT INTO learning_custom_questions`，当前 `.bind(now)`）。
- 实现为 `scheduler.rs` 新增辅助函数 `first_review_due_at(now, tz) -> TimestampMs`。

### 4.2 理由

- 当天刚阅读过课文并完成练习题，同一批题目立即再次出现属于简单重复，记忆收益低；次日 02:00 统一出现，用户感知即"明天刷"。
- 与打卡锁定制配合：今天到期数量的变化来源从三种（重学卡、新题、跨日滚动）减为仅 sub-day 重学卡一种。
- 对 FSRS 无影响：首刷时 `last_reviewed_at = None`、`days_elapsed = 0`，FSRS 按新记忆状态计算；首复时刻从"当天"推迟到"次日 02:00"不改变记忆状态输入。

### 4.3 边界与页面状态

- `00:00–01:59` 入库的卡（复习日 = 前一天）在**当日 02:00** 到期——即新的一天开始即到期，Anki 同款语义；熬夜用户 02:00 后可刷，次日清晨打开也已到期。
- 「问题管理」页状态不变：`review_count == 0` 且课时已完成 → "待首复习"，只是不再当天进入队列；「今日复习」角标数量随之减少（新卡不再当天计入）。

## 5. 每日打卡

### 5.1 目标设置

- 用户可设置每日目标张数 N，存 `client_preferences`，key：`learning.dailyCheckinGoal`。
- 默认值 15；`N = 0` 表示"仅清空队列"（无数量目标，刷完即完成）。
- 设置入口：学习设置页「每日打卡目标」输入项（已实现，存 `learning.dailyCheckinGoal`，范围 0–500）。

### 5.2 完成条件与锁定

打卡状态按**复习日**结算，完成判定为瞬时快照：

- 条件 A：当日（当前复习日）累计复习 ≥ N（N > 0 时）。
- 条件 B：判定时刻无到期卡（`due_at <= now` 的卡数量为 0）。
- 任一满足即完成，写入 `learning_checkins` 锁定当天；锁定后当天新增的到期卡（sub-day 重学卡、新题）**不改变**打卡状态，但继续出现在复习队列，作为"额外任务"可刷。
- 完成是自动判定的（查询时推导 + 落表），不需要用户点击"打卡"按钮；如后续要仪式感按钮，仅作为强制锁定的入口，不改变判定规则。
- 跨复习日自动进入新一天：`02:00` 后查询即返回新复习日的未完成状态。

### 5.3 统计口径

- "当日累计复习" = 当前复习日区间内提交的所有复习（含 sub-day 重学卡；重学卡计入已刷数量，鼓励完成重学而非惩罚）。
- 统计源：新表 `learning_review_events`（实现时确认 `learning_attempts` 不可用：它同时承载课程练习且无 `user_id` 列，无法区分练习与复习）。每次 `answer_review` / `answer_custom_review` 在事务内写入一行（`source` = course/custom，`item_id` = 卡片 id，`created_at`），按 `user_id + created_at >= 复习日 02:00` 聚合即得当日已刷数。`learning_checkins` 只记录锁定结果与当日目标，不承担明细计数。

### 5.4 存储

新表 `learning_checkins`（`nomifun-db` 追加式 migration）：

| 列 | 类型 | 说明 |
|---|---|---|
| `checkin_id` | TEXT PK | UUID |
| `user_id` | TEXT | 所属用户，与 `(user_id, review_day)` 建唯一索引 |
| `review_day` | INTEGER | 复习日（实现时选定一种整型表示，与 `review_day(t)` 一致） |
| `goal` | INTEGER | 完成时的目标 N（快照，后续改目标不回溯） |
| `reviewed_count` | INTEGER | 锁定时刻的累计复习数 |
| `completed_at` | INTEGER | 锁定时刻（UTC 毫秒） |

- 幂等：`(user_id, review_day)` 唯一，重复写入按 INSERT OR IGNORE / ON CONFLICT 处理。
- 不做打卡连续天数（streak）计算，留给后续功能。

复习事件表 `learning_review_events`（同一 migration）：`event_id`（UUID PK）、`user_id`、`source`（`'course'` / `'custom'` CHECK）、`item_id`（复习卡 id）、`created_at`（UTC 毫秒）；`(user_id, created_at)` 建索引供按复习日聚合。

### 5.5 接口契约

`GET /api/learning/checkins/today`（路径以 routes.rs 现有挂载前缀为准，实现时对齐）：

响应（示意 JSON）：

```json
{
  "review_day": 20260817,
  "goal": 15,
  "reviewed_count": 12,
  "due_count": 5,
  "completed": false,
  "locked_at": null
}
```

- `completed` 由判定逻辑实时推导并落表：返回前执行一次判定，满足则写入 `learning_checkins` 并置 `completed = true`、`locked_at = now`。
- `due_count` = 当前到期卡数（`due_at <= now`，课程卡 + 自定义题）。
- 前端展示：进度条锚定 `reviewed_count / goal`（固定目标，不随队列增长而膨胀）；`due_count` 单独展示（"还剩 X 张，其中新增 Y 张"可选，本期只要求分别展示）。

## 6. 验证与测试计划

### 6.1 scheduler 单元测试（`scheduler.rs`）

| 用例 | 断言 |
|---|---|
| 跨 02:00 边界 | `01:30` 复习的 day-level 卡，到期日 = 当天（即前一天复习日 + round(interval)）的 02:00 |
| `00:00–01:59` 属前一天 | `01:00` 复习：`review_day` 为昨天；新卡入队 due = 当天 02:00 |
| 白天复习 day-level | `10:00` 复习 interval=1：due = 次日 02:00（不再漂移到次日 10:00） |
| sub-day 不抹平 | interval=0.5 天：due = now + 12h（精确），即使跨过 02:00 也不改 |
| `days_elapsed` 复习日差 | `23:50` 与次日 `23:40` 复习：差值为 1（旧整除算法为 0） |
| 新卡首复 | `first_review_due_at`：白天入库 due = 次日 02:00；`00:00–01:59` 入库 due = 当日 02:00 |
| 时区偏移 | 同一 UTC 时刻、不同 tzOffset，到期日不同（东八区与 UTC 各验证一例） |

### 6.2 打卡单元测试（`service.rs`）

| 用例 | 断言 |
|---|---|
| 刷满锁定 | 当日累计复习达到 N 的瞬间，`completed = true` 且落表 |
| 清空锁定 | 到期数为 0（含 N = 0 场景）即完成 |
| 锁定后新增不影响 | 锁定后插入一张 `due_at = now` 的新卡，状态仍为已完成，`due_count` 增加 |
| 跨日重置 | 复习日切换后查询返回新复习日未完成状态 |
| 幂等 | 同 `(user_id, review_day)` 重复判定不重复插行 |

### 6.3 回归

- 现有 `schedule_review` 全部测试用例语义保持（首次复习 ≥ 1 天、Again 短间隔、Hard/Good/Easy 单调性、desired retention 敏感性），仅到期时刻的断言按新规则修正。
- 前端「今日复习」「问题管理」的队列查询契约不变，无前端回归风险（本期无 UI 改动）。

## 7. 实现要点（代码落点）

| 文件 | 改动 |
|---|---|
| `crates/backend/nomifun-learning/src/scheduler.rs` | 新增 `review_day(t, tz)`、`review_day_start_02_00(day, tz)`、`first_review_due_at(now, tz)`；`schedule_review` 增加时区/复习日参数，day-level 走"到期日 02:00"，sub-day 保持精确；`days_elapsed` 改复习日差 |
| `crates/backend/nomifun-learning/src/service.rs` | `scheduler_settings()` 增读 `learning.tzOffsetMinutes`；两处 `submit_review` 传时区；课时完成入队与手动加题改调 `first_review_due_at`；打卡判定与 `learning_checkins` 读写 |
| `crates/backend/nomifun-learning/src/routes.rs` | 挂载 `GET /api/learning/checkins/today`（路径与现有前缀对齐） |
| `crates/backend/nomifun-db/migrations/` | 追加 migration 建 `learning_checkins` 与 `learning_review_events`（复习事件明细，打卡统计源） |
| 前端（本期仅实现设置项，其余契约） | 启动时上报 `learning.tzOffsetMinutes`；学习设置页已实现目标 N（默认 15）；今日复习页进度条与徽章另排期 |

依赖：时区计算使用 chrono（`FixedOffset`），确认 `nomifun-learning` 可直接依赖或经 `nomifun-common` 复用，遵循"backend 依赖走 workspace 根 `Cargo.toml`"的约定。

## 8. 假设与未决事项

1. **sub-day 重学卡保持精确时刻**（记忆科学要求）。若未来要求"当天不再出现任何卡"，需另行评估：遗忘后等待时间最长 14h+，重学强化效果显著下降。
2. **时区采用前端上报存偏好**，桌面端与 Web 端均准确；未上报回退服务器本地时区（Web 端多时区用户需前端上报后生效）。
3. **打卡按复习日结算**，跨日（02:00）自动进入新一天。
4. **新卡首复按复习日语义（+1 复习日）**，凌晨 0:00–2:00 入库的卡在当日 02:00 到期属预期行为。
5. 打卡连续天数（streak）、成就、提醒推送不在本期范围。
6. 打卡完成后的"额外任务"是否在 UI 上做视觉区分（如"额外"角标），本期只要求可刷，不做标记。

## 9. 不做的事（本期范围外）

- 不改队列查询语义（`due_at <= now` 三处共用保持）。
- 不迁移存量 `due_at`（自然过渡）。
- 不做前端 UI 实现（本 spec 只定后端规则与接口契约）。
- 不为 sub-day 增加"至少 24 小时"下限（会破坏遗忘重学）。
- 不引入"至少 24 小时"day-level 下限（实际间隔 22–48h 已在 FSRS 取整噪声内，无意义）。
