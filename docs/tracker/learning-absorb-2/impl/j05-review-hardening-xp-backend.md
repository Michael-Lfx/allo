---
id: j05
title: "复习事件加固 + XP 结算后端"
type: task
status: closed
assignee: zhangyj
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
- [x] **review_log 加四列**（推进前快照，全 nullable、永远落盘 null 不省略）：
      `elapsed_days/stability_before/difficulty_before/r_pred`——推进时从卡片当前状态
      铸值（「事后不可复算：参数重训会污染回放」）
- [x] **同日折叠语义钉死**：同卡同学习日（02:00 翻转沿用）重复推进 = 只记作答流水不
      推进不落日志（CONTEXT.md 既有语义）——补测试钉死；作答流水行补 `elapsed_ms/
      response/session 锚/forgot/predicted`（g4 数据面，同批迁移）
- [x] **种卡幂等显式化**：已存在 = 跳过；异常冲突 fail loud + 测试
- [x] **结算事件表** `learning_xp_events`（追加只增）：`ts/course_id/node_id?/kind
      ('node-completed'|'review-completed'|'quiz-completed')/earned_xp/xp_bonus/basis
      ('full'|'partial'|'trimmed'|'guess-penalty')/ratio?/origin?/anchor`；**写侧条款
      validate_settlement 照抄**（full ⇒ 非负且 ≥ bonus 无 ratio；partial ⇒ ratio ∈ [0,1]
      必带无 bonus；trimmed ⇒ 恰 0；guess-penalty ⇒ 必为负；quiz 恒非负——条款先入）
- [x] **settle 纯函数包**（零 IO）：教新基数 = est 分钟；复习基数 = Σ 完成题数 × 题型
      当量（选择/判断 1；填空/数值/匹配/排序 2；简答/开放/AI 批改 3——起步值待校准）
      下限 3；分级授予 award（全对 +bonus 2 / 部分 `(base×ratio).floor()` / 砍短 0 /
      乱猜：`n≥4 && actual < 0.5×baseline` → `−max(base÷2,1)`，文本基线 0.25；AI 批改轨
      入账按 3 分钟档）；**幂等键**：教新 = 节点一次性、复习 = 课程×节点×学习日一笔
- [x] **companion 桥**：结算落账与 `add_companion_xp` 同步调用（事件 id 幂等去重；
      companion 写失败不回滚学习账，滞后由后续结算补偿；Lv 读 companion 总额）
- [x] **读数端点**：`GET /xp/reading`（today_xp/goal/goal_achieved/settled_nodes/
      total_nodes/remaining_est/eta_days——两把尺同源回放互不换算、ETA 绝不回写；进度尺
      分子分母按现存且启用同筛）+ `GET /xp/calendar?weeks=12`（恒 84 桶含零桶、末桶 =
      今日、负账如实、不过课程筛）；日目标 40 住应用设置（1..=1000 越界拒绝）
- [x] **打卡口径退役（后端）**：`dailyCheckinGoal` 计数口径停用（UI 随 j06）；学习日
      02:00 翻转沿用
- [x] **乱猜常量集中表**：`GUESS_MIN_QUESTIONS=4 / GUESS_SIGNIFICANCE=0.5 /
      GUESS_PENALTY_DIVISOR=2 / GUESS_TEXT_BASELINE=0.25 / REVIEW_MIN_MINUTES=3` +
      「报警阈值可调」注记
- [x] learning-smoke 增 `xp` 读数面子命令（结算几笔 → 读数/日历打印；读数面非门禁）
- [x] 验证：`cargo test -p nomifun-learning`（learnhub 口径：四形态写侧条款红绿样 /
      分级授予阶梯 / 乱猜判据 / 幂等键 / 尺永不倒挂）+ `cargo test -p nomifun-companion`
      （桥幂等）

**Resolution:**

**速览**：三件加固（快照 052 已落列只补测试钉死；折叠语义既有只补测试；作答流水补五
列）+ XP 结算全后端（事件表 + settle 纯函数包 + 写侧条款 + companion 桥 + 读数/日历 +
打卡口径退役）一次落地。两线先行（教新/复习），quiz 线只入条款与 kind 枚举（载体随
g4）。

1. **快照四字段**：核实 `learning_review_log` 四列 052 已建、`record_review_log` 已按
   「推进前铸值、无状态落 NULL 不省略」写侧就绪——本票补测试钉死（首推四字段全 NULL
   + elapsed_days 0；重推逐字段断言从卡片状态铸值）。
2. **作答流水补五列**（078 同批迁移）：`learning_review_events` 增
   `elapsed_ms/response/session_id/forgot/predicted`——session_id = g4 数据面前置列
   （会话实体缺位恒 NULL，将来升会话即 ULID 锚）；课程路径与 attempt 同事务写全量锚
   （predicted 与复习日志同一铸值口径），自建题路径 elapsed/predicted 缺席 NULL。
3. **折叠**：语义既有（`push_allowed` 到期门 + 同日推进查重），补折叠测试：同卡同学习
   日重复作答 → 作答流水照记、调度不动、无新日志行、无新结算。
4. **种卡幂等**：`ensure_review_item` 已存在 = 跳过（调度原样，不静默重置）+ 测试；插
   入撞 `UNIQUE(enrollment_id, activity_id)`（并发种卡竞态）= 显式 Conflict fail loud
   （防御分支，测试口径为幂等跳过 + 迁移唯一约束地板）。
5. **`learning_xp_events`**（078）：追加只增；`earned_xp` 定为**净入账**（full 时含
   bonus 分量）——条款「full ⇒ 非负且 ≥ bonus 无 ratio」即 earned ≥ xp_bonus，与
   learnhub 账面同构；表级 CHECK 同条款地板（四形态 + quiz 恒非负 + ratio [0,1] +
   bonus 仅 full）；`UNIQUE(user_id, anchor)` 幂等兜底；v3 契约全量注册
   （PRODUCT_TABLES/UUIDV7/event_id 豁免/user→users Cascade、course/node→历史面
   KeepHistory 容缺失父行、表计数 114→115）。
6. **settle 纯函数包**（`settle.rs` 零 IO）：教新 `settle_node`——基数 est、授予比例 =
   passed/total（未作答拉低比例）、乱猜实际正确率 = passed/answered、乱猜先行于阶梯、
   无练习课时 = full 无 bonus、零通过 = trimmed；复习 `settle_review`——基数
   max(题型当量, 3)、得分 1 = full+2、0 = partial ratio 0、(0,1) = partial floor。
   **当量表注记**：多选归 2 档（票面「选择」按认知负荷扩义：多选需构造完整答案集合，
   与填空同级）；AI 批改当量 3（复习线题型限制实际不出现 AI 题，quiz 线载体生效）。
   **复习幂等键注记**：allo 复习队列为题级卡，「课程×节点×学习日」的节点 = 卡
   （review:course:{item_id}:{学习日} / review:custom:{question_id}:{学习日}）。
7. **写侧条款 `validate_settlement`** 照抄四形态 + quiz 恒非负（条款先入，检测线后置
   防带病上线），红绿样 6 + 10 条；落账唯一写点 `record_settlement` 条款先入、
   INSERT OR IGNORE 兜锚、0 值事件只落账不过桥。
8. **结算挂钩**：教新 = `update_lesson_progress` Completed 后（重复完成被锚吸收）；
   复习 = 三条真实推进路径（`rate_review`/`rate_custom_inner`/`answer_review`
   auto-lapse）提交后结算——自评 again = 0 分、hard/good/easy = 全对。
9. **companion 桥**：trait 缝 `CompanionXpSink`（learning 侧零依赖，缺省无桥只落学习
   账）→ 组合根实现（`LearningCompanionXpBridge`）→ `CompanionService::
   grant_learning_xp`（生效默认伙伴；花名册空如实 false = 滞后容忍）→ store 单事务
   （`companion_learning_xp` 去重行 + XP 增量同生共死，事件 id 幂等，失败整体回滚可
   安全重试；负数结算照实扣减）；桥失败仅 warn 不回滚学习账。测试：同事件重复授予只
   入账一次、负结算如实、空花名册不落账。
10. **读数/日历**：`GET /api/learning/xp/reading` 与 `/calendar`——两把尺同源回放互不
    换算；进度尺分子分母同筛现存且启用（报名课程、未删墓碑、enabled=1）；ETA = 近 7
    学习日日均 XP 为步伐（同量纲不换算），空账 null、学完 0、纯投影绝不回写；日历恒
    weeks×7 桶含零桶、末桶今日、负账如实、不过课程筛。日目标 `learning.dailyXpGoal`
    缺省 40，读侧越界回退、写侧 nomifun-system `validate_value` 拒绝 1..=1000 之外。
11. **打卡口径退役（后端）**：`checkin_today` 改 XP 口径——goal = dailyXpGoal、
    `reviewed_count` 字段承载今日净入账（字段名保留到 j06 换 UI 数据源）、锁定快照记
    XP 口径、清空队列完成条件退役（`dailyCheckinGoal` 读取路径删除）；学习日 02:00
    翻转沿用；旧测试按新口径重写（2 删 2 改 1 增）。
12. **smoke `xp`**：临时库真机走通——种单课时（est 25 + 单选）→ 练习全对完成（教新
    full 27）→ 卡到期答对自评 good（复习 full 5）→ 读数（today_xp 32、settled 1/1、
    remaining 0、ETA 0）+ 84 桶日历打印；读数面非门禁。
13. **测试**（learnhub 口径 +20）：settle 单测 7（条款红绿 6+10 / 阶梯 / 乱猜先行与
    n=4 边界 / 当量表）、服务 7（教新 est 计价与幂等锚 / 复习折叠与一笔结算 / 快照
    逐字段 / 自建题无课程锚 / 种卡幂等 / 尺永不倒挂 / XP 打卡锁定）、迁移 4（条款
    地板 / 锚唯一 / 作答流水列）、桥幂等 1、dailyXpGoal 值域门 1。
14. **验证**：`cargo test -p nomifun-learning` 全绿、`cargo test -p nomifun-db` 全绿
    （含 id_schema_contract 23 + 078 迁移 4）、`cargo test -p nomifun-companion` 除
    存量失败外全绿（`run_for_skips_when_model_unconfigured` /
    `learn_endpoint_is_companion_scoped…` 与 `nomifun-system` 的
    `managed_provider_secret_is_redacted…` 均在干净 HEAD f918470cf 复现，与本票无
    关）、smoke xp 真机 32 XP、`bun run typecheck` 60 错 = 干净 HEAD 存量基线（增量
    0）、`check:i18n` 过（11392 键）、`cargo check --workspace` 干净、`cargo fmt`
    按包过。
