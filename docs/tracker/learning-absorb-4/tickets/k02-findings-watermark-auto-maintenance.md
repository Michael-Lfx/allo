---
id: k02
title: "findings 消费水位 + 定时自动维护回合（先修补后挂载、零工作不提交、归档课不扫）"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# k02: findings 消费水位 + 定时自动维护回合

**What to build:** j09 落的 `learning_structure_findings` 只生产不消费——每次修补
回合全量重读全部历史 findings。吸收 learnhub #146/#147（ADR-0028）：给 findings
加**消费水位**（成功回合才推进），并加**定时自动维护回合**（先修补后挂载、
纯读判定、零工作不提交、归档课不扫），设置开关 + 间隔可配。

## allo 现状

- findings 表：迁移 `068_learning_domain_rebuild.sql:597-618`（append-only，
  `id, ts, course_id, source, node, field, issue, advice`）。
- 唯一生产者 `record_review_findings`（`service/learning_graph.rs:927-962`，
  growth_body 评审后调用）；唯一消费者 = `run_structure_repair`
  （`learning_graph.rs:663-685`）**全量读取**（`ORDER BY ts, id`，无水位、
  无删除、无标记消费）。前端零读取。
- 挂载/修补编排：`run_structure_mount`（:326-591，slot
  `learning-mount-{course}`、kind `structure_mount`）、`run_structure_repair`
  （:598-922，slot `learning-repair-{course}`、kind `structure_repair`）；
  WS `structure_mount_*` / `structure_repair_*` 已有；任务注册表
  `service/generation_tasks.rs:28-30`。
- 调度先例：`nomifun-app-server/src/lib.rs:2905-2947`
  `start_marketplace_auto_update`——`tokio::spawn` + interval tick、每 tick
  重读配置、单轮在途。
- 设置先例：`learning.dailyXpGoal` 写侧 `nomifun-system/src/client_pref.rs:226-236`
  validate_value（1..=1000 拒绝）+ 读侧 `service/xp.rs:220-239`（越界回退缺省）。
- `learning_courses.enabled`（068:442，缺省 1）——归档课过滤就绪。
- 开关先例：`service/generate.rs:43-59` SOURCE_PLAN_KILL_SWITCH。

## learnhub 参考（ADR-0028 + #146/#147）

- `src-tauri/src/app/maintenance.rs`：`spawn()` sleep-then-work 循环（:330-355，
  每醒重读设置=下周期生效）；`run_tick()` 四闸各自一行跳过（:291-325）；
  `tick_with()` 纯读扫描 → **先修补后挂载**（:195-279），busy = 跳过含挂载
  （:214-221），**零工作不提交**（:177-188 一行 info）；`maintenance_needs()`
  纯函数（:103-107）：`needs_repair = 未消费 findings>0 || mounts_stale`；
  `needs_mount = mount_pending || mounts_stale`；归档过滤 `filter(enabled)`
  （:155-157）。
- 水位：`engine/maintenance/findings.rs`——水位文件存消费行数
  （:150-168 缺失/损坏=0+warn；:175-203 跳过已消费前缀、越界钳制；
  :207-219 原子写）；**成功回合才推进**（applied :385-390 / noop :307-312；
  skipped/failed 不推，测试 :1322/:1361）。
- 设置：默认开、间隔 360 分钟、边界 30..=10080（越界收编不报错）
  （`infra/settings.rs:47-63`）。

## 实施要点

- [ ] **迁移 070**（高危区纪律：迁移测试 + v3 契约注册同步）：
      `learning_maintenance_state(course_id PK→learning_courses, findings_consumed_id
      INTEGER NOT NULL DEFAULT 0, updated_at)`——一行一课。水位 = 已消费的最大
      finding `id`（SQLite 行自增单调，等价 learnhub 行数水位且免坏行处理；
      坏行/越界语义 = 钳制到 max(id) 不阻断）。
- [ ] 修补回合读侧改水位化：`run_structure_repair` 输入 findings 改
      `WHERE id > :watermark ORDER BY id`；**回合终态 applied/noop 才推进水位**，
      failed/skipped/cancelled 不推（失败回合的 findings 仍是下次输入）。
- [ ] `maintenance_needs` 纯函数（零 LLM 纯读）：needs_repair =
      未消费 findings > 0 || 挂载 stale；needs_mount = 待挂集非空 || stale
      （复用 j08 `build_mounts_view` 三态/pending 投影）。
- [ ] 调度循环 `start_learning_auto_maintenance`（住 app-server，marketplace
      先例同款）：默认开、间隔缺省 360 分钟、每醒重读设置；逐 enabled 课程
      纯读判定 → 需要则先 `run_structure_repair` 后 `run_structure_mount`
      （busy/失败跳过该课含挂载）；零工作零任务零 LLM；单轮在途防重入。
- [ ] 设置两键：`learning.maintenanceAutoEnabled`（bool，缺省开）+
      `learning.maintenanceIntervalMinutes`（30..=10080，缺省 360；写侧拒绝
      + 读侧回退，dailyXpGoal 同款）；设置页两控件 + i18n 双语。
- [ ] 两枚开关：`NOMI_LEARNING_FINDINGS_WATERMARK=off`（修补读全量=旧行为、
      不推水位）与 `NOMI_LEARNING_AUTO_MAINTENANCE=off`（调度器不注册）。
- [ ] WS 复用既有 `structure_repair_*`/`structure_mount_*` 事件，零新事件面。

## 验收

- **单测（红绿）**：种子 findings → 修补回合 applied → 水位推进、二回合输入
  为空 → skipped 零 LLM 零写；失败回合（completer 报错）水位不动；水位越界
  （手工写超 max id）钳制不炸；off 臂读全量。
- **db 测试**：迁移 070 + v3 契约注册全绿。
- **smoke 新档 `maintenance`**：temp SQLite 种子课程 + findings + stale 挂载；
  硬判据（EXIT=0）：纯读判定正确（needs 读数行）、回合 1 消费并落审计、
  回合 2 skipped 零 LLM 零写、归档课（enabled=0）不在扫描面、间隔/开关设置
  生效。无凭据时回合 1 如实非零（mount/repair 档先例）。
- **双臂对照（开关臂，同二进制）**：off 臂两回合均全量重读（findings_read
  读数两轮相等、二回合重跑 LLM）；on 臂二回合零调用——对照卡落 Resolution
  （读数行：findings_read / llm_calls / rounds_run × 两臂并排）。
- `cargo test -p nomifun-learning` + `-p nomifun-db` 全绿；i18n/check 门过。

## 非目标

- GUI「跑维护回合」按钮与两段结果条（learnhub maintenance.tsx）——后端端点
  已有（mount/repair 路由），按钮位随走查另议（map 雾区）。
- findings 列表端点/前端读面；findings 排序消费策略（j10 雾项另议）。

## 备注

- 挂载观察（mount observations）的派生触发（j09：仅在挂载行存在时派生）不动；
  本票只加 findings 水位与调度。
- 调度器对 web 与 desktop 两宿主同一生效（app-server 共用）。

## Resolution

**已关（2026-10-23，`3e3b67126`）**。

- **迁移 070**：`learning_maintenance_state(id PK AUTOINCREMENT, course_id UNIQUE,
  findings_watermark INTEGER NOT NULL DEFAULT 0 CHECK ≥0, updated_at)`。列名由
  `findings_consumed_id` 改为 `findings_watermark`——v3 契约只支持 TEXT 逻辑
  引用，`_id` 后缀会被判为关系列；语义不变。注册 `PRODUCT_TABLES` + 逻辑引用
  （Cascade，留痕本身 KeepHistory）；迁移测试 + `validate_id_schema_contract`
  全绿。
- **水位化读侧**：`run_structure_repair` 只读 `id > watermark ORDER BY id`；
  applied/noop 推进到本回合最大 id（钳制 `MAX(id)`，只升不降 upsert）；
  failed/skipped/cancelled 不推；off 臂（`NOMI_LEARNING_FINDINGS_WATERMARK=off`）
  读全量且不推。
- **needs 纯读**：`maintenance_needs`（eligible + unconsumed_findings +
  pending_mounts + mounts_stale + needs_repair/needs_mount），归档/软删/非图/
  无结构 = ineligible。
- **回合与调度**：`run_maintenance_tick`（先修补后挂载、修补失败/在途跳过该课
  含挂载、修补后重判定、零工作不提交）；app-server
  `start_learning_auto_maintenance`（30s 唤醒、每醒重读设置、单轮在途、
  `NOMI_LEARNING_AUTO_MAINTENANCE=off` 不注册），在 routes 启动处接线。
- **设置**：`learning.maintenanceAutoEnabled`（bool 缺省开）+
  `learning.maintenanceIntervalMinutes`（30..=10080 缺省 360）；写侧拒绝、读侧
  回退；设置页两控件 + i18n 双语；`gen:i18n`/`check:i18n` 过。
- **smoke `maintenance` 档**：新 CLI/命令；判据按开关分支（on：水位推进 +
  二回合零工作零 LLM；off：不推水位 + 二回合重跑）。
- **双臂（开关臂）**：on `watermark=Some(2)`、tick2 idle=1 worked=0、LLM 2→2；
  off `watermark=None`、tick2 worked=1、LLM 2→3；两臂 EXIT 0、归档课
  scanned 恒 1。读数见 [verification-cards.md](../assets/verification-cards.md) §k02。
- **单测**：水位推进/二回合 skipped/失败不推/越界钳制/off 臂/端到端 tick/
  设置回退（learning 483→488 全绿）+ db 迁移/契约测试。
- 非目标遵办：GUI「跑维护回合」按钮未做（map 雾区）。
