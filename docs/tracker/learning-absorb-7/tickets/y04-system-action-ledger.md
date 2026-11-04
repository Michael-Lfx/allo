---
id: y04
title: "系统动作簿——自主动作终态留痕 + 课程详情折叠节"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# y04: 系统动作簿

**What to build:** 给学习图课程的自主/后台动作（维护修补、维护挂载、手动
修补/挂载、生长补批）落一条**终态**账：谁触发、依据读数、结果、失败原因、
关联对象。写入 fail-open（账本失败不阻断动作），读侧按课程倒序，课程详情
「学习记录」加一个折叠节「系统动作」。**只吸语义，不搬 JSONL 账本/全档扫描。**

## 现状（2026-11-04 实码核实）

- `service/generation_tasks.rs:340-390` 写 `learning-tasks.jsonl` 的 begin 行、
  `:494-520` 写 finish 行；**无读 API、无前端消费**；取消不写 finish
  （`:535-570`），注册表重启即清。JSONL 由
  `nomifun-app/src/services.rs:2958-2971` 落到
  `<data_dir>/observation/learning-tasks.jsonl`。
- 生长批 `learning_growth_batches`（迁移 070:225-253）：有 seq/status/note，
  **无触发源**；成功 applied、失败 failed，**取消也写 failed**
  （`service/learning_graph.rs:1626-1630,1694`）；前端把取消显示成红色
  「失败」（`LearningGraphWorkspace.tsx:708-712`）。
- 维护回合 `run_maintenance_tick`（`service/learning_graph.rs:1347-1440`）
  的报告 `MaintenanceTickReport`（`models.rs:3057-3080`）只在内存，除日志/
  smoke 外无消费者；无 HTTP 路由、无落表。
- 修补行 `learning_structure_repairs` 只在 `!noop` 时插入（`:966,985`）；
  挂载只有当前态 `learning_structure_mounts`，无历史；skipped 只进 JSONL。
- 前端只有批次时间线（`LearningGraphWorkspace.tsx:657-718`）与生成 pill，
  无维护/挂载/取消面。

## learnhub 参考（main@22d016d）

- `src-tauri/src/app/action_log.rs:75-168`：`ActionRecord{at, kind, course,
  trigger(timer|manual|session_settlement), basis, result(done|failed{reason}),
  shadow}`；一次动作只写一行**终态**；写失败只 warn；读侧按课程过滤、时间
  倒序、截 limit（默认 200）。
- 写入点：`app/maintenance.rs:246,284,317,343`（timer 修补/挂载完成与失败）；
  `app/commands.rs:424-470`（手动动作成功 Done、同键忙拒/用户取消不写、真失败
  Failed）；`app/sessions.rs:1754,1770`（结算补批成功/失败，取消不写）。
- 展示：`list_system_actions` 命令（`commands.rs:482-497`）+ 课程详情折叠节
  `system-actions.tsx:32-93`（时间+类型+触发源+basis+结果徽标，失败
  `role=alert` 带 reason；空态「暂无系统动作」）。
- `22d016d` 的四项评审修复：忙拒/取消不落失败、读命令结构化日志、重答入口、
  写点口径（蓝图称七点、实码 10 点）——allo 对应处理见下。

## 实施要点

### A. Schema（编辑 070 基线，不新增迁移文件）

- [ ] 新表（草案，字段可按实现微调）：
  ```sql
  CREATE TABLE learning_system_actions (
      id            INTEGER PRIMARY KEY AUTOINCREMENT,
      action_id     TEXT NOT NULL UNIQUE,   -- ULID/36 位，与现有 id 校验同形
      course_id     TEXT NOT NULL,
      kind          TEXT NOT NULL,           -- maintenance_repair|maintenance_mount|growth
      trigger       TEXT NOT NULL,           -- timer|manual|settlement|auto_refill
      basis         TEXT NOT NULL DEFAULT '',-- 一句判据读数（如 needs/ready 数）
      result        TEXT NOT NULL,           -- done|failed
      reason        TEXT NOT NULL DEFAULT '',-- failed 原因；done 为空
      ref_id        TEXT NOT NULL DEFAULT '',-- batch_id / repair 行 id / mount 行 id
      actor_user_id TEXT,                    -- manual 动作的操作者；timer 为 NULL
      at            INTEGER NOT NULL
  );
  CREATE INDEX idx_learning_system_actions_course
      ON learning_system_actions (course_id, at);
  ```
  枚举集不设 DB CHECK（ADR-0015）；`action_id` 校验沿用现有 36 位 GLOB。
- [ ] `id_schema_contract.rs`：`PRODUCT_TABLES` 加表；identity 登记
  `(learning_system_actions, action_id)`；`course_id` 作为 `text_ref!` 到
  `learning_courses`（Cascade，索引同上）；补迁移测试。
- [ ] `learning_growth_batches.status` 增加 `cancelled`（Rust 单源，无 DB
  CHECK）；`GraphBatchView` 注释与前端 tag 支持中性「已取消」；取消不再写
  failed。历史行不回填（旧取消仍显示 failed，如实记档）。

### B. 写侧（fail-open，单源 helper）

- [ ] `service` 新增 `record_system_action(NewSystemAction)`：写失败只
  `tracing::warn!`，**不 `?`、不阻断调用方**；时间戳 ms；basis 截断 ≤200 字。
- [ ] 写点清单（票面要求实现时用测试钉住，防文档漂移）：
  1. `run_maintenance_tick`：每课每次修补/挂载终态一行；`skipped_busy`/
     零工作不写；失败写 `failed` + reason；`basis` 带 `needs`/`recheck`
     读数（如 `unconsumed=2 pending_mount=1 stale=0`）。
  2. `run_structure_repair` / `run_structure_mount`：手动触发成功 Done、
     真失败 Failed；同键忙拒（`Conflict`）与用户取消**不写**。内部增加
     `trigger` 参数（tick 传 `timer`，路由传 `manual`）；公共签名可保留
     默认 manual 包装，避免大改调用点。
  3. `kick_growth`：成功 applied → Done；真失败 → Failed；取消 → **不写**
     （批行写 `cancelled`）。`trigger` 由调用方传：结算
     `maybe_kick_growth` → `settlement`；POST grow 默认 `manual`；前端
     自动补批请求可带可选 query `trigger=auto_refill`（缺省 manual，向后
     兼容）。
  4. 修补 noop / 挂载 skipped：零工作不写（与 learnhub A3 一致）；若排障
     需要，只在 tracing 留痕，不进账本。
- [ ] 文档 `docs/architecture.md` 增「学习系统动作簿」节：数据形状、写点
  清单、fail-open、与 generation task/批次状态机的边界（进行中看任务 pill，
  终态看账本）；顺手修正现有漂移注释（修补 noop 落行、failed=生长失败、
  begin/finish 两型）。

### C. 读侧与前端

- [ ] `GET /api/learning/courses/{id}/system-actions?limit=`（默认 50，上限
  200）：按 `at DESC` 返回 `SystemActionView`；结构化 info 日志带
  `course_id/limit/returned`；课程不存在/软删 404。
- [ ] UI：课程详情「学习记录」时间线顶部/末尾加可折叠「系统动作」节
  （`LearningGraphWorkspace.tsx` 的 `GraphHistory` 旁），行 = 时间 + 类型 +
  触发源 + basis + 结果徽标；失败红色 `role=alert` 带 reason；空态
  「暂无系统动作」；`cancelled` 批次用中性 tag。i18n zh-CN/en-US 双写 +
  `bun run gen:i18n`。
- [ ] 新 API 加入 `ui/src/renderer/pages/learning/api.ts` 与类型
  `types.ts`（手写契约，与 Rust DTO 对齐）。

### D. 测试

- [ ] 服务：helper 写失败不阻断（注入坏 DB/约束冲突）；忙拒/取消零行；
  维护 tick 每动作一行、零工作零行；growth 取消批行 `cancelled` 且零账本行。
- [ ] 路由：倒序/limit/上限/404；结构化日志（可测字段）。
- [ ] 迁移：表/索引/identity 登记；`cargo test -p nomifun-db`。
- [ ] UI：空态、失败 reason、cancelled 中性 tag；typecheck/i18n。

## 验收

- `cargo test -p nomifun-learning -p nomifun-db` 全绿；`bun run typecheck`；
  `bun run check:i18n`。
- 真机（串行）：跑一轮 maintenance tick（至少一次修补或挂载）→ 课程详情
  能看到对应行与 basis 读数；手动取消一次生长 → 批次显示「已取消」且账本
  无失败行；smoke `EXIT=0`。

## 非目标

- JSONL `action-log` 单文件账本、全档扫描、`shadow`/`Started` 预留位。
- 新 WS 通道/phase；账本做执法或阻断前置（保持 fail-open 旁路）。
- 回填历史动作（trigger 无法从旧行可靠推导；只从上线后新动作起记）。
- 把 generation task 的进行中状态搬进账本（进行中看任务 pill）。

## 风险

- 账本与 `learning_growth_batches`/`learning_structure_repairs` 有重叠：
  必须用 `ref_id` 关联、不复制载荷，避免 note/status 两处漂移。
- 维护每课每动作一行会随课程数增长；读侧按课程 + limit，必要时加保留策略。
- 多用户：`actor_user_id` 只记手动操作者；课程共享，账本不按用户过滤
  （避免他人维护动作不可见）。若产品要求按用户隔离，另立票。
- `cancelled` 是新状态值：所有 match `status` 的前端/后端必须穷举（Rust
  enum/字符串比较），用测试钉住。

## Resolution（2026-11-04）

**实现点**

- Schema（编辑 `070_learning_domain_rebuild.sql` §13，未新增迁移文件）：
  `learning_system_actions`（`action_id` UUIDv7 唯一业务身份、`course_id`
  逻辑引用 Cascade、`actor_user_id` → users KeepHistory、`ref_id` 多态不复制
  载荷）+ 读侧索引 `(course_id, at)` + actor 索引。枚举三组（kind/trigger/
  result）Rust 单源在 `learning_graph::system_action`，DB 无 CHECK。
  列名用 `trigger_source` 避开 v3 契约禁止的 `TRIGGER` 物理令牌（视图字段
  仍叫 `trigger`）。`learning_growth_batches.status` 增加 `cancelled`
  （Rust 单源，无 DB CHECK）。
- `id_schema_contract.rs`：`PRODUCT_TABLES` + identity
  `(learning_system_actions, action_id)` + `ref_id` 非引用列 + 两条逻辑引用；
  全库契约校验通过。
- 写侧：`service/system_actions.rs` 的 `record_system_action`（fail-open，
  写失败只 warn；basis 截断 ≤200 字；`NewSystemAction` builder）。写点：
  维护 tick 修补/挂载（timer）、手动修补/挂载（manual）、生长
  applied/failed（manual/settlement/auto_refill）。noop/skipped/idle/忙拒/
  取消零行；生长取消把批行写 `cancelled`（不再写 failed）。
- 读侧：`GET /api/learning/courses/{id}/system-actions?limit=`（缺省 50、
  上限 200、按 `at DESC, id DESC`），课程不存在/软删 404；结构化 info 日志
  `course/limit/returned`；POST grow 新增可选 `?trigger=auto_refill`（缺省
  manual，仅改标签不改裁决）。
- 前端：学习记录顶部可折叠「系统动作」节（NomiCollapse；失败红 + role=alert
  + reason，空态「暂无系统动作」）；批行 `cancelled` 中性灰 tag；自动补批
  hook 传 `trigger=auto_refill`；纯投影函数在 `systemActions.ts` 并单测。
- 文档：`docs/architecture/learning.zh.md` 新增「学习系统动作簿（y04）」节
  （数据形状/写点清单/fail-open/与任务注册表与批行状态机的边界），并修正
  漂移注释：修补审计行只在 applied 时落行（noop/skipped 不落）、
  failed=真失败而 cancelled 是中性终态、learning-tasks.jsonl 仅 begin/
  finish 两型且无读 API/无前端消费。

**测试命令与结果**

- `CARGO_TARGET_DIR=… cargo test -p nomifun-learning`：790 passed, 0 failed。
- `CARGO_TARGET_DIR=… cargo test -p nomifun-db`：全部 suites passed（新增
  `learning_system_actions_migration` 4 tests；`id_schema_contract` 更新表数
  契约为 125）。
- `cargo fmt -p nomifun-learning -p nomifun-db -p nomifun-common -- --check`：
  clean。
- `bun run typecheck`：55 条存量错误，学习模块 0 条、无新增。
- `cd ui && bun test src/renderer/pages/learning`：93 passed, 0 failed
  （含新增 `systemActions.test.ts` 8 项）。
- `bun run gen:i18n && bun run check:i18n`：up to date（11574 keys）。

**偏差**

- 表列名 `trigger` → `trigger_source`：v3 schema 契约禁止物理 `TRIGGER`
  令牌（`id_schema_contract` 运行期扫描），DTO/前端字段仍为 `trigger`。
- 票面文档路径 `docs/architecture.md` 实为 `docs/architecture/learning.zh.md`
  （仓库无根级 architecture.md），已写在该域文档。
- 真机 smoke（EXIT=0）未在本 worktree 执行——需要真实 provider 凭据与串行
  环境，留给集成验证轮。
- 取消/忙拒状态下"零账本行"由测试钉住；旧版本已写下的 failed 取消行不回填
  （票面明示如实记档）。
