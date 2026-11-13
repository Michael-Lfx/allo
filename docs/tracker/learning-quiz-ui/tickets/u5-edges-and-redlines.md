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

已收口（2026-11-11）。R1–R6 全落地，R7 部分完成（见下）。

### 评审与修复（R6）

双轴评审（Standards/Spec 并行、独立上下文）→ 4 处必修单代理一次收口
（e7ed96692 + f0a1aea75，交付合并 4ee392c9c）：

- Standards：硬违规 0（三红线全过）；judgement call 5 处——死代码兜底
  （本波修）、阈值缺钉值（本波修）、initTestI18n 拷贝趋势 / DTO 落位存量
  形态 / 会话接线风格分叉（三项记录在案不扩波）。
- Spec：u5 R1–R4 三处未真落地（组卷等待文案 / 会话内 409 闭环 / 异常回合
  状态门）+ u2 兜底链与真实接线脱节成死代码——全部修复：`quizPreparing`
  等待文案（R1）、409 → `quizNotDueHint`（K 从报文提取，漂移回落
  conflictBusy）+ `onConflict` 通道 bump 进度令牌 + 关会话（R2）、
  `round.status !== 'active'` 门 + `quizRoundNotActive` 引导（R3）、
  QuizEntryCard 简化为同步回调（R4）、阈值常量 export + 0.9/0.6/0.3 钉值
  （R5）。scope creep 零（两条后置项零偷做）。

### 红线全闸（R5）

`bun run check` 聚合闸 exit 0（typecheck · i18n · theme · button-layout ·
icons · dead-css · codemirror · 各 repo 级闸 + Agent Store 闸 + script
registry）；learning 目录回归 385 pass / 0 fail。

### 真机走查（R7）——部分完成，点击级走查留用户

dev:web + 浏览器自动化（IAB）走查 launch-sim 造数库：

- **T1 通过**（DOM + 截图双证据）：学习页「今日复习 8 张」横幅、XP 日历
  4 天 × 54、课程卡 8/8 + 到期 8 + 累计 XP 216；课程工作区知识检测区块
  「距上次检测已积累 8/6 次学新结算」+「开始检测」可点，due 态呈现与
  K 后端单源全部符合。
- **T2–T5 运行时受阻**：IAB webview guest 中途死亡（guest not attached，
  恢复链：页签重绑/面板重建/新建页签/等 30s 重试全失败；ZCode 宿主侧
  工具故障，**非应用缺陷**）。后端侧核实零副作用（progress 仍 8/6 due）。
  开测→判分→结算→归零的点击级走查留用户桌面执行（清单见
  learning-launch-sim 票 Resolution quiz 段，造数态已就绪且未被消费）。
- 环境准备留痕：`seed-learning-triggers.mjs --fresh`（8/6 due）+
  `bun run dev:web`（5173/8787，insecure-no-auth）；测试后已停服释放库锁。
