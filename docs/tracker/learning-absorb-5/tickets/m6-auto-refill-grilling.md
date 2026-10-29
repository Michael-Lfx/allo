---
id: m6
title: "自动补批定案——生长作为调度动作（#121/ADR-0024 域中立）"
type: grilling
status: closed
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# m6: 自动补批定案（grilling）

**Question:** learnhub #121 把隐式生长提为一级调度动作（自动补批：就绪+到期
不足 K 时自动跑一批）。allo 已有两处生长触发（手动 HTTP + 课时完成进度钩子），
但没有「面不够就补」的裁决函数与开关。本票盘问：allo 的触发点、阈值、暂停
线怎么定，裁完切实施票。

## allo 现状

- 生长入口：① 手动 `POST grow`（routes.rs:284-297 → `kick_growth(user, course,
  true, None)`）；② 进度钩子 `maybe_kick_growth`（service/learning_graph.rs:1394，
  service/progress.rs:100 在课时完成/跳过后调）。
- `kick_growth`（:1350-1388）：`require_live_graph_course` :1359 → 就绪触发门
  `READY_TRIGGER` :1362 → 停滞臂（仅自动，ADR-0010）:1365 → 课级 slot 锁
  `graph-growth-{course}` :1371-1378 → `tokio::spawn` :1383。
- 调度器先例（k02）：`start_learning_auto_maintenance`（nomifun-app-server
  lib.rs:2736-2820，30s tick 重读设置 → `run_maintenance_tick` 逐课纯读判定
  → 需要才提交，busy 跳过）——自动补批的第二触发点可寄宿于此或另起。
- 设置先例：client_preferences KV + validate_value（dailyXpGoal /
  maintenance 两键先例）。
- 任务注册表：generation_tasks 键级互斥，无并发上限（m7 领地——本票补批若
  落地，任务提交面应走 m7 资源闸；依赖方向在票内注记）。

## learnhub 参考（ADR-0024 + c2967cf 修订 + 770cfd4 修复）

- 机制终态：`auto_refill_verdict`（coach.rs:2624-2686，纯读零 LLM）——
  **阈值先行**（就绪候选 + 到期 < `AUTO_REFILL_THRESHOLD=2` → Due），
  **暂停线次之**（评审债 ≥ `AUTO_REFILL_PAUSE_LINE=40` → Paused，在
  REVIEW_DEBT_LIMIT 80 之前一档——复习优先，生成是投资不是救济），否则
  BelowThreshold；preview 恒关闸。
- 触发两时点：① start_session（resolve → AutoRefill → 注册 grow 任务 →
  重解析；重入集 `refilled: BTreeSet` 每课每调用至多一批，二次裁决如实回
  「已在准备中」）；② 教新结算后（`auto_refill_enabled && Due` →
  fire-and-forget 线程；读失败 = Paused fail-closed 不破坏结算）。
- 防失控四件（c2967cf 撤销日预算后）：**并发 1 / 开关 / 暂停线 / 重入集**。
  日预算闸被撤销的教训：正常「长批→跳批→再长批」循环会被预算过早拦截。
- 开关 `auto_refill_enabled` 默认开；裁决日志带 ready/due/debt 数字
  （770cfd4：关键路径状态变迁行不可缺）。
- 770cfd4 另一条：开始屏非就绪态保留开始动作（自动补批派发口在
  start_session，空态只挂通知 = 「自动准备中」承诺无人兑现）。

## 盘问点（裁决产出物）

1. **触发点②怎么域中立**：allo 无 start_session 教练会话——候选：
   (a) 寄宿 k02 维护 tick（逐课纯读判定时顺带补批判定——但维护 tick 语义
   是「修补/挂载」，混入生长改变票面语义，需要 ADR 注记）；
   (b) 进度钩子已覆盖「完成时点」（maybe_kick_growth），第二时点改为
   「工作区打开/课程页加载」（前端调一个只读裁决端点，Due 则触发）；
   (c) 只做完成时点 + 手动（最小面）。
2. **阈值与暂停线的 allo 量纲**：learnhub K=2（就绪+到期）、暂停线 40（评审
   债）。allo 的对应物：就绪候选 = READY_TRIGGER 语义？到期 = 什么（allo 无
   复习队列到期概念于图课程——XP 面 due_today？评审债 = findings 未消费数？
   停滞臂数？）——盘问定。
3. **开关与默认**：`learning.autoRefillEnabled` 默认开否（learnhub 默认开）；
   与 NOMI_LEARNING_* kill switch 的分层（设置面 vs 环境闸）。
4. **与 k02 维护 tick 的关系**：同一循环还是两个循环（tick 预算/互斥）。
5. **m7 依赖方向**：补批提交走资源闸（若 m7 未落地，本票先记接缝）。

## 验收（本票 = 定案）

- Resolution 载：触发点/阈值量纲/暂停线/开关默认/循环归属五项裁决 + 切出的
  实施票（含 verdict 纯函数单测红绿、smoke 双臂方案——off/on 臂裁决读数、
  对照卡 = 补批前后 ready/due 计数并排）。
- learnhub 参考先读铁律：定案前至少读 auto_refill_verdict + start_session
  补批循环 + ADR-0024 全文（含修订注）。

## 备注

- learnhub 教训两条直接继承：日预算闸撤销（过度拦截正常循环）、「自动准备中」
  空承诺（派发口必须真实可达）。
- allo 的 READY_TRIGGER/停滞臂已有语义，verdict 必须与之**组合**而非旁路
  （盘问点 2 的核心）。

## Resolution（2026-10-26 grill-with-docs 定案，用户按推荐全部裁决）

- **触发两时点**：① 既有完成/跳过检查（`maybe_kick_growth`，就绪<3 门）保持；
  ② **课程工作区加载只读裁决端点**（`GET /courses/{id}/growth/verdict` 纯读，
  Due 则前端自动调既有 POST grow 后台补批，「自动准备中」承诺由真实派发口
  兑现——770cfd4 教训）。不寄宿 k02 维护定时器（越过 ADR-0010「收尾交学习者」
  精神且混维护票面语义）。
- **阈值维持 READY_TRIGGER=3 不动**；m6 只把触发包装成显式 verdict 函数
  （Due/BelowThreshold/Disabled）+ 带数字裁决日志（ready/due）。**无独立暂停线**
  ——allo 无评审债量纲，不硬搬 learnhub 40（量纲必须有自己的源）；防失控四件
  = 并发 1（课级 slot 锁既有）+ 开关 + 重入集（每课每裁决轮至多一批）+ m7
  资源闸。
- **开关**：`learning.autoRefillEnabled` 默认开 + `NOMI_LEARNING_AUTO_REFILL=off`
  环境兜底，两件分层（用户面 vs 灭火面）。
- **依赖**：实施票 m10 `blocked_by: [m7]`（补批提交走资源闸）。
- **术语落盘**：CONTEXT.md 新增 Auto Refill 词条；Growth 词条批容量 7→5 修正
  （对齐代码 READY_TARGET=5 / MAX_BATCH_NODES=5）。
- **事实修正**：盘问中核实「完成时点 + 就绪<3 自动检查」已存在
  （`maybe_kick_growth` → `kick_growth` :1362 门）——本票增量仅为 verdict
  显式化、第二触发点、开关与重入集，非从零设计。
- **实施票**：m10（自动补批落地）。
