---
id: j05
title: "复习事件加固 + XP 结算后端"
type: task
status: open
assignee:
blocked_by: [j01, j02]
part_of: ../map.md
---

# j05: 复习事件加固 + XP 结算后端

**What to build:** 复习日志三件加固（快照四字段/折叠测试/种卡幂等）+ XP 结算全后端
（事件表 + settle 纯函数 + 写侧条款 + companion 桥 + 读数/日历端点 + 打卡口径退役）。

**实现决议:** 三轮 t3 + 三轮 t1 + 二轮 g1（后端面）。参考实现：learnhub
`engine/questions/settle.rs`、`infra/ledger/event.rs`、`domain/xp.rs`。

**Blocked by:** [j01]（est 命名——工作量当量以 est 计价）、[j02]（进度尺筛现存且启用）。

**Status:** ready-for-agent

- [ ] **review_log 加四列**（推进前快照，全 nullable、永远落盘 null 不省略）：
      `elapsed_days/stability_before/difficulty_before/r_pred`——推进时从卡片当前状态
      铸值（「事后不可复算：参数重训会污染回放」）
- [ ] **同日折叠语义钉死**：同卡同学习日（02:00 翻转沿用）重复推进 = 只记作答流水不
      推进不落日志（CONTEXT.md 既有语义）——补测试钉死；作答流水行补 `elapsed_ms/
      response/session 锚/forgot/predicted`（g4 数据面，同批迁移）
- [ ] **种卡幂等显式化**：已存在 = 跳过；异常冲突 fail loud + 测试
- [ ] **结算事件表** `learning_xp_events`（追加只增）：`ts/course_id/node_id?/kind
      ('node-completed'|'review-completed'|'quiz-completed')/earned_xp/xp_bonus/basis
      ('full'|'partial'|'trimmed'|'guess-penalty')/ratio?/origin?/anchor`；**写侧条款
      validate_settlement 照抄**（full ⇒ 非负且 ≥ bonus 无 ratio；partial ⇒ ratio ∈ [0,1]
      必带无 bonus；trimmed ⇒ 恰 0；guess-penalty ⇒ 必为负；quiz 恒非负——条款先入）
- [ ] **settle 纯函数包**（零 IO）：教新基数 = est 分钟；复习基数 = Σ 完成题数 × 题型
      当量（选择/判断 1；填空/数值/匹配/排序 2；简答/开放/AI 批改 3——起步值待校准）
      下限 3；分级授予 award（全对 +bonus 2 / 部分 `(base×ratio).floor()` / 砍短 0 /
      乱猜：`n≥4 && actual < 0.5×baseline` → `−max(base÷2,1)`，文本基线 0.25；AI 批改轨
      入账按 3 分钟档）；**幂等键**：教新 = 节点一次性、复习 = 课程×节点×学习日一笔
- [ ] **companion 桥**：结算落账与 `add_companion_xp` 同步调用（事件 id 幂等去重；
      companion 写失败不回滚学习账，滞后由后续结算补偿；Lv 读 companion 总额）
- [ ] **读数端点**：`GET /xp/reading`（today_xp/goal/goal_achieved/settled_nodes/
      total_nodes/remaining_est/eta_days——两把尺同源回放互不换算、ETA 绝不回写；进度尺
      分子分母按现存且启用同筛）+ `GET /xp/calendar?weeks=12`（恒 84 桶含零桶、末桶 =
      今日、负账如实、不过课程筛）；日目标 40 住应用设置（1..=1000 越界拒绝）
- [ ] **打卡口径退役（后端）**：`dailyCheckinGoal` 计数口径停用（UI 随 j06）；学习日
      02:00 翻转沿用
- [ ] **乱猜常量集中表**：`GUESS_MIN_QUESTIONS=4 / GUESS_SIGNIFICANCE=0.5 /
      GUESS_PENALTY_DIVISOR=2 / GUESS_TEXT_BASELINE=0.25 / REVIEW_MIN_MINUTES=3` +
      「报警阈值可调」注记
- [ ] learning-smoke 增 `xp` 读数面子命令（结算几笔 → 读数/日历打印；读数面非门禁）
- [ ] 验证：`cargo test -p nomifun-learning`（learnhub 口径：四形态写侧条款红绿样 /
      分级授予阶梯 / 乱猜判据 / 幂等键 / 尺永不倒挂）+ `cargo test -p nomifun-companion`
      （桥幂等）

**Resolution:**（收口时写）
