---
id: t1
title: "XP 结算落地定案（二轮 g1 的实现参考）"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: map.md
---

## Question

二轮 g1 已裁「完整采纳：宣言 + 统一账 + 三线 + 全式含惩罚 + 打卡并入 XP + est 统一」
未实施；learnhub #88 现在有完整参考实现（settle.rs 纯函数包 + validate_settlement 写侧
条款 + xp.rs 读数投影 + 84 格日历）。本票把 allo 落地的剩余实事定案：

1. **九题型当量表**：learnhub 是选择 1/短答 2/开放 3 分钟（复习工作量当量）；allo 九题型
   （ordering/matching/numeric/fill-in/AI 批改轨等）逐型定当量值（学习成本 × 认知负荷
   分级，起步值标注待校准——learnhub 亦自标「起步值待校准」）。
2. **AI 批改轨是否入账**：开放题 AI 判分的作答算不算「完成题数」——learnhub 文本题基线
   0.25 保守上界的同款问题；建议入账、判分口径与乱猜基线对齐 AI 批改的置信特性（实施时
   定，先裁出入账与否）。
3. **结算事件落点**：二轮已裁「统一账：学习结算入 companion XP」——事件流表放 learning
   库还是 companion 侧、`add_companion_xp` 的桥接形态（learnhub 是「数值由调用方算好，
   落账不计算」+ 写侧条款拦截——条款随事件走，落点跟事件）。
4. **读数与日历**：两把尺（进度 = 结算数 / XP = 努力账）+ ETA + 84 格日历（含零桶、
   空日 UI 留空）的 DTO 与端点；打卡计数口径退役后的 UI 数据源切换。
5. **乱猜判据参数**：≥4 题、< 0.5×基线、罚 基数÷2、文本基线 0.25——常量集中表起步值
   照抄 + 「报警阈值可调」注记。
6. **「现存且启用」口径联动**：进度尺分子分母同筛、日历不过筛（三轮 g3 裁决联动）。
7. **冒烟**：learning-smoke 是否加 xp 读数面（对齐常设偏好）。

产出：以上各项定案（含当量表初值），供二轮 + 三轮合并切票施工。参考实现：learnhub
`engine/questions/settle.rs`、`infra/ledger/event.rs`（validate_settlement）、`domain/xp.rs`。

## Resolution

**速览**：七项定案——结算事件表落 learning 库 + companion 同步桥；两线先行（教新/复习），
检测线载体随 g4 后置；九题型当量表三档起步；读数与 84 格日历全回放派生。

1. **九题型当量表（起步值，标注待校准）**：对齐 learnhub 1/2/3 分钟骨架按认知负荷分档——
   选择/判断 = 1；填空/数值/匹配/排序 = 2；简答/开放/AI 批改 = 3。常量集中表 +
   「起步值待校准」注记（learnhub 同款）。
2. **AI 批改轨入账**：作答算完成题数（工作量当量按 3 分钟档）；乱猜基线沿用文本保守
   上界 0.25（「宁可漏判不可误伤」）；AI 判分的 ratio = 判分得分率。
3. **结算事件落点**：learning 库新表 `learning_xp_events`（追加只增：`ts, course_id,
   node_id?, kind('node-completed'|'review-completed'|'quiz-completed'), earned_xp,
   xp_bonus, basis('full'|'partial'|'trimmed'|'guess-penalty'), ratio?, origin?, anchor`
   ——幂等键：教新 = 节点完成一次性事实；复习 = 课程×节点×学习日一笔）；**写侧条款
   `validate_settlement` 照抄**（full/partial/trimmed/guess-penalty 四形态 + quiz 恒非负
   ——检测线后置但条款先入，防将来带病上线）。**统一账桥**：结算落账与
   `add_companion_xp` 同步调用（同一函数内，事件 id 幂等去重；companion 写失败不回滚
   学习账——以 learning_xp_events 为学习线真相，companion 余额允许滞后，重试由下次结算
   自然补偿；Lv 读 companion 总额）。
4. **读数与日历**：`GET /api/learning/xp/reading`（today_xp/goal/goal_achieved/
   settled_nodes/total_nodes/remaining_est/eta_days——**两把尺同源回放互不换算**，ETA
   绝不回写）+ `GET /api/learning/xp/calendar?weeks=12`（恒 84 桶含零桶、末桶 = 今日、
   负账如实出桶）；日目标 40 住应用设置（范围 1..=1000 越界拒绝）；打卡计数口径退役，
   日历/读数 UI 换 XP 数据源（二轮 g1）。
5. **乱猜判据常量**：`GUESS_MIN_QUESTIONS=4`、`GUESS_SIGNIFICANCE=0.5`、
   `GUESS_PENALTY_DIVISOR=2`、`GUESS_TEXT_BASELINE=0.25`、`REVIEW_MIN_MINUTES=3`——
   常量集中表 + 「报警阈值可调」注记。
6. **现存且启用联动**（g3）：进度尺分子分母同筛；日历/总账不过筛。
7. **冒烟**：learning-smoke 增 `xp` 读数面子命令（结算几笔 → 读数/日历快照打印；
   读数面非门禁——json 子命令同款）。

**两线先行的范围注记**：检测/quiz 线载体随 g4 后置（雾区），教新线 + 复习线一次到位；
learnhub `settle.rs` 的 award 阶梯/乱猜判据/写侧条款为本票的全部参考代码。

