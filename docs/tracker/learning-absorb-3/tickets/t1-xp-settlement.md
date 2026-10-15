---
id: t1
title: "XP 结算落地定案（二轮 g1 的实现参考）"
type: task
status: open
assignee:
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
