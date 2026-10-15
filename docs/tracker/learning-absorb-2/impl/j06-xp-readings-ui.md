---
id: j06
title: "XP 读数/日历 UI + 打卡退役"
type: task
status: open
assignee:
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

- [ ] 今日 XP 读数卡：`today_xp / goal / goal_achieved / eta_days`（「只报安排与事实，
      不承诺记忆效果」措辞纪律）
- [ ] 学习日历 84 格（近 12 周，末桶 = 今日）：**零桶显式在场、空日 UI 留空不填 0**；
      双主题语义令牌；悬浮提示逐日 XP
- [ ] 打卡 UI 退役：`dailyCheckinGoal` 计数组件/入口移除，位置由 XP 读数接管（i18n 键
      清理双向对称）
- [ ] 进度尺读数（已结算 / 总节点数——现存且启用口径，j02 单源）并入课程/总览合适位置
- [ ] 验证：`bun run typecheck`（learning 路径零新增）+ `bun run check:i18n` +
      `bun run check:theme` + `bun run check:dead-css`（无新增 learning 项）

**Resolution:**（收口时写）
