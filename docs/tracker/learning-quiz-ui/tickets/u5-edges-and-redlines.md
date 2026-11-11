# u5: 边界态 + 红线全闸 + 双轴评审 + 真机走查

## Status

施工中（2026-11-11 开票）。

## Terms

- **R1（start 慢路径等待态）**：有凭据时 `quizStart` 内联跑变式链 LLM
  （VARIANT_V2 证据门；零凭据诚实跳变式只出 bank 旧题）——开测呈现诚实
  loading（不可取消：后端无取消端点，不造假取消）；失败呈现错误 + 可重试；
  等待文案注明「正在组卷」。smoke 实测变式链耗时窗口参考 learning-smoke
  quiz 臂。
- **R2（409 未 due 竞态）**：u2 兜底之外的会话容器内路径——start 返回
  409 Conflict（`quiz not due: N/K settlements since last quiz`）→ 不崩、
  不假成功：呈现 N/K 未到期文案 + 触发 progress 刷新关闭会话。
- **R3（supersede 语义呈现）**：旧 active 回合被新 start 顶替是后端语义
  （`status: abandoned`）；UI 不做回合恢复入口（learnhub 记录文件面后置
  同源）；若 start 返回 completed/abandoned 状态的回合（异常路径）如实
  呈现并引导回课程页，不进答题流。
- **R4（complete 兜底）**：缺题拒绝（400）是后端兜底——UI 前置校验
  （u3 R5）之外的残留路径（如变式题 activity_id 漂移）呈现错误 toast +
  保留作答态不丢卷。
- **R5（红线全闸）**：`bun run check` 全绿——i18n 双语零漏（check:i18n）、
  主题双态（check:theme）、dead-css 零警告、button-layout、icons；
  `bun run typecheck` 绿；新增组件带渲染测试（due 两态/判分反馈/结算
  分级/折叠 null）。
- **R6（双轴评审）**：本波五票双轴评审一轮（机制轴：契约对齐后端 DTO/
  红线不破；呈现轴：i18n/主题/布局），硬违规清零收口。
- **R7（真机走查）**：launch-sim 造数库全链目验并记录 Resolution——
  `bun scripts/seed-learning-triggers.mjs --fresh` 喂 due（8/6）→ 课程详情
  页入口可点 → 开测（凭据臂验变式等待态；无凭据臂验 bank 题诚实跳过）→
  答题判分（对/错两态 + 判底/解析/证据）→ 结算页（总入账 + 逐节点对账 +
  分级）→ 完成回课程页 N/K 归零 → XP 日历出现 quiz-completed 入账 →
  打卡锁定链路（复习 + quiz 过 40 线锁日）。

## Acceptance

- R5 全闸绿 + R6 评审硬违规清零。
- R7 真机走查记录（含截图/读数）进 Resolution；发现的缺陷独立立票。
- 走查清单同步更新 launch-sim 票 Resolution 的 quiz 段（API 级 → 点击级）。

## Resolution

（施工后收口）
