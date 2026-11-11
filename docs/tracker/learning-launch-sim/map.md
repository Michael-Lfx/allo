# Map: 上线前触发数据模拟——复习/测验免等待验证

<!-- wayfinder:map -->
<!-- 状态：s1 已关（2026-11-11）——脚本 + 走查清单交付，副本验证全绿；
      真机走查待用户执行。 -->
<!-- 来源：上线前真机验证需求——复习卡首到期 = 次日本地 02:00
      （first_review_due_at），测验 = 6 节点结算触发（QUIZ_TRIGGER_
      SETTLEMENTS），两条线在干净开发库上都要等真实时间/真实积累才能
      触发。本波造一个开发库造数脚本 + 真机走查清单。 -->

## Destination

`bun scripts/seed-learning-triggers.mjs` 一键把开发库（`Flowy/Nomi-dev/
flowy-backend.db`）喂到复习/测验即刻可触发状态：8 张到期卡（含推进史）、
8 节点结算散布过去 4 天（XP 日历 + 打卡史）、quiz due 翻转；真机走查清单
随票 Resolution 交付。

## Notes

- **机制事实源**（2026-11-11 勘察）：
  - 复习：`ensure_review_item`（progress.rs）种卡，首到期次日 02:00；队列
    `due_reviews` 只读 `due_at <= now` + 未归档 + 课程启用未删。
  - 测验：`quiz_settled_nodes` 数 `node-completed` 事件（锚
    `node:{lesson_id}` 唯一），≥6 翻转 due；SPA 无 quiz 面，API 级验证。
  - 打卡：j05 起 XP 口径——日 XP ≥ 目标（缺省 40）锁日；02:00 翻转。
  - 幂等锚：`UNIQUE(user_id, anchor)`；全部学习 ID 是 UUIDv7 严格 CHECK。
  - 时区：`client_preferences.learning.tzOffsetMinutes`，缺省机器本地偏移。
  - 先例：冒烟基建早已直改库（learning_smoke.rs `UPDATE learning_review_
    items SET due_at`）；种课 INSERT 形态以 learning_smoke quiz 阶段为范。
- **已知偏离（如实声明）**：造数走账本直插，不过 companion XP 桥——companion
  余额不会有这批历史 XP，学习账面（日历/打卡/触发）不受影响；今日 XP 从真
  实复习/测验结算开始积累。
- **纪律**：脚本只碰开发库（默认路径防呆）；写前 `VACUUM INTO` 自动备份
  （锁探测二合一，后端在跑即失败并提示）；`--fresh` 清后重造；幂等可重跑。

## Tickets

| 票 | 标题 | 依赖 |
|---|---|---|
| [s1](tickets/s1-seed-learning-triggers.md) | 造数脚本 + 真机走查清单 | 无 |
