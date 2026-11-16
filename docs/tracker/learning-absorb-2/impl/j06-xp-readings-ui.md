---
id: j06
title: "XP 读数/日历 UI + 打卡退役"
type: task
status: closed
assignee: zhangyj
blocked_by: [j05]
part_of: ../map.md
---

# j06: XP 读数/日历 UI + 打卡退役

**What to build:** 学习线努力账的前端面——今日 XP 读数与目标、84 格学习日历、打卡计数
UI 退役换 XP 数据源。

**实现决议:** 二轮 g1（前端面）+ 三轮 t1 读数消费。参考：learnhub `xp-calendar` /
总览入口（「非 streak 不做激励」「空日留空不填 0」）。

**Blocked by:** [j05]（读数/日历端点与打卡口径退役后端）。

**Status:** ready-for-agent

- [x] 今日 XP 读数卡：`today_xp / goal / goal_achieved / eta_days`（「只报安排与事实，
      不承诺记忆效果」措辞纪律）
- [x] 学习日历 84 格（近 12 周，末桶 = 今日）：**零桶显式在场、空日 UI 留空不填 0**；
      双主题语义令牌；悬浮提示逐日 XP
- [x] 打卡 UI 退役：`dailyCheckinGoal` 计数组件/入口移除，位置由 XP 读数接管（i18n 键
      清理双向对称）
- [x] 进度尺读数（已结算 / 总节点数——现存且启用口径，j02 单源）并入课程/总览合适位置
- [x] 验证：`bun run typecheck`（learning 路径零新增）+ `bun run check:i18n` +
      `bun run check:theme` + `bun run check:dead-css`（无新增 learning 项）

**Resolution:**

**速览**：打卡面整体换血——`CheckinPanel`（今日打卡卡 + 月历/年热力图）退役，新
`XpPanel`（今日 XP 卡 + 进度尺行 + 84 格 XP 日历）接管其位置；横幅徽章与折叠条改 XP
措辞；设置页 `dailyCheckinGoal` → `dailyXpGoal`（1..=1000 对齐后端写侧门）；i18n 退役
32 键、新增 20 键双语对称。

1. **今日 XP 卡**：数据 = `checkins/today`（j05 起 XP 口径：goal = XP 目标、
   reviewed_count = 今日净 XP、completed = 达标锁定）为主，`xp/reading` 补充实时读数；
   进度条 + 达标 ✓ + 「还差 N XP」+ 调整目标链接。完成仪式（celebrateToken 高亮动画）
   保留——庆祝的是「今日 XP 达标」。措辞纪律照抄：只报已得/目标/还差，不承诺记忆效果。
2. **进度尺行**（读数并入总览位置）：`settled_nodes/total_nodes`（现存且启用双筛，
   j02 单源）+ `remaining_est` + `eta_days`（现步伐约 N 天 / 暂无步伐信号）——纯事实
   陈述，一行三读数并入 XP 卡下方；课程级进度尺已由 j02 的 CourseSummary 承担，本票
   只补总览面。
3. **84 格 XP 日历**：`GET /xp/calendar?weeks=12`（后端分桶，前端零时区换算——旧面板
   的 tz_offset 显式传参形态随打卡计数口径一并退役）。7 行 × N 列对齐星期（周日起），
   首列前置空位，末桶 = 今日（primary 描边）；**零桶显式在场、空日留空不填 0**——有
   结算的日子染色显数（四级 primary 色阶），无结算日子只留 faint 底格、不渲染 0；
   **负账如实出桶**（danger 染色 + 图例「负结算（乱猜罚金）」）；悬浮逐日 XP
   （`M 月 D 日 · ±N XP / 无结算`）。色阶与文字反白阈值与色阶绑定，双主题全语义
   令牌（`--color-fill-1` / `rgb(var(--primary-6))` / `--danger-6` / color-mix，与退役
   面板同词汇，check:theme 过）。
4. **打卡退役面**：横幅紧凑徽章改 XP（`今日 XP {{xp}}/{{goal}}` / `✓ {{xp}}/{{goal}}`，
   「无目标」分支随 goal ≥ 1 的后端口径消亡）；折叠条标题/展开收起文案改「XP 读数」；
   **月历/年热力图整体退役**（复习次数强度热力 = 计数口径的一部分；learnhub xp-calendar
   的替换形态，「非 streak 不做激励」——连续天数展示一并退役）。`stats/calendar` 端点
   与 CalendarStats 类型保留（后端历史面不动，账本历史投影归后续票裁决），UI 消费清零。
   `useCheckinStatus` hook 保留（轮询对象即 XP 口径），注释对齐。
5. **设置页**：`learning.dailyCheckinGoal`（0..=500 计数）→ `learning.dailyXpGoal`
   （1..=1000，缺省 40，与后端写侧拒绝门同界）；configKeys 双注记更新。
6. **i18n 双向对称清理**：退役 checkin* 32 键（徽章 4 / 卡片 5 / 月年视图 8 / 图例
   7 / 悬浮 6 / streak 1 / 折叠 1），新增 xp* 20 键（面板 3 / 卡片 3 / 进度尺 4 /
   日历 10）；键数 11392 → 11380；gen:i18n 重生成，check:i18n 过。
7. **验证**：`bun run typecheck` 60 错 = 存量基线（learning/XpPanel/ReviewBanner/
   settings 零新增）；`check:i18n` 过（11380 键对称）；`check:theme` 过；`check:dead-css`
   失败项全在未触碰的非 learning 文件（基线唯一 learning 项 QuestionManager 仍有效，
   learning 零新增）；`bun test src/renderer/pages/learning/` 19 过。
