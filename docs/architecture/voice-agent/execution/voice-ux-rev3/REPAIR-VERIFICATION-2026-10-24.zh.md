> **2026-10-25 宿主启动行为验收更新**：已补齐真实文件库、OS 服务锁和 `AppServices → create_router` 组合启动测试；旧构造期提前结算的临时退化触发 3/3 行为失败，源码逐字恢复后 boot 84 项通过。此前“仅有源码接线证据”的对应缺口已关闭，原生/设备与二进制冷启动仍单列；本节优先于下方历史描述，提交状态以 Git 为准。详见 [宿主启动验证记录](HOST-BOOT-VALIDATION-2026-10-25.zh.md).

> **2026-10-25 父审查最终独立验收**：R1–R3 及两轮补修定向通过；父审查重编译原始 SQLite 探针 3/3，正式定向测试 139/139。执行者全量日志 1677 项另行核对，未冒充父审查全量复跑。完整宿主启动、T4 DOM/原生/设备仍未签核；AEC/自然插话本轮未实现。未提交、未推送，半小时监控在复审完成后停止。详见 [独立复审报告](OPENCODE-INDEPENDENT-REVIEW-2026-10-25.zh.md).

<a id="second-followup-2026-10-25"></a>

## 2026-10-25 第二次补修：精确代次清理权限与长历史完整性
独立复审以真实 SQLite 探针确认上一补修仍有两个缺陷（`independent-review-second-probes.txt`：1 pass / 2 fail / 7 filtered）。本节事实优先于下方所有“已关闭”结论。

### ① 过期 A 清理越权触碰 B
- 反例：真实 claim/admit A → exact finalize A → claim/admit B → exact finalize B → 设置 B session → 用 A 的旧 epoch/operation 调 cleanup；结果 `Ok(AlreadyApplied)` 且 `later session=None`。
- 根因：exact helper 底部对 `status=finished`、无 active/fence 的旧回执吸收路径返回 `AlreadyApplied`，新增的清理写入未校验 epoch。旧回执吸收语义不等于写当前消息/session 的权限。
- 修复：`apply_restart_orphan_artifacts` 仅在 `transition == AlreadyApplied && epoch == expected_admission_epoch`（以及既有精确 `Committed` 分支）执行；过期代次不再触碰 B。普通 exact-finalize 的回执吸收/返回契约保持不变；无需依赖跨代 `AlreadyApplied` 再清理补偿。
- 正式测试：`stale_cleanup_cannot_touch_a_later_finished_generation_or_its_session` 与 `stale_cleanup_cannot_touch_a_later_running_generation` 与 `stale_cleanup_cannot_touch_a_later_active_null_generation`，验证 B 的 session、thinking/work 消息、admission epoch/active operation 均不变；RED 为 finished B session 被清空，GREEN 3 passed。

### ② 长历史尾部断流消息遗漏
- 反例：同一真实 Conversation 写入 5001 条较早完整消息，最新 interrupted thinking 在尾部。proof-aware reconcile 返回 `Reconciled`，但 thinking 仍 `work`、ACP session 仍在。
- 根因：`prepare_proven_interrupted_turn_artifacts` 只取 `get_messages(..., 1, 5000, Asc)` 第一页并当作完整历史；终态后不再扫描。
- 修复：按现有 `get_messages` 分页（5000/页）遍历至 `has_more=false`，逐页只收集 thinking/work 清理写入；空非末页或任意页读取失败返回 `Conflict`，在事务开始前 fail-closed，不产生半套 lifecycle 清理。未新增迁移、表、配置或后台恢复系统。
- 正式测试：`boot_reconcile_covers_transcript_tail_beyond_one_recovery_page`，5001 条历史 + 尾部 thinking；RED 为 session 未清，GREEN 验证 session 清空、thinking `done` 且原文保留。

### 回归
- `boot_reconcile_sqlite`：**14 passed / 0 failed**（`second-followup-boot-reconcile.txt`）。
- DB 两 crate all-target：**47 targets / 1677 passed / 0 failed / 0 ignored**（`second-followup-db-conversation-all.txt`）；Conversation lib 648、DB lib 458、schema 23。
- host `boot_terminal_proof` 10 passed；R2 `durable_voice_identity...` 1 passed；`cargo check -p Flowy` exit 0；`git diff --check` 与改动文件 rustfmt exit 0。
- 保留上一补修的 ACP/thinking/work 失败回滚、无证明/remote/过期证明/健康 session 全部断言，未新增 ignored/skip。

### 证据边界
`router/state.rs` 的 `include_str` 断言只验证源码接线顺序（AppServices 不早期结算、proof provider 先于 sweep），**不等于完整 AppServices→router 行为集成**；行为证据是真实 SQLite 的恢复/回滚与真实 host provider/registry 测试。T4 真实 DOM/原生/DPI/设备与 AEC 仍未验。


<a id="followup-2026-10-25-cleanup"></a>

## 2026-10-25 follow-up：恢复清理丢失的原子修复（优先于下方）
独立复审用真实 SQLite 反例确认：首轮在 `finalize_exact_cancelled_turn_generation` 提交 `finished/active=None` 后才 best-effort 清理。若 ACP clear 失败或两步间崩溃，下一次 boot 不再枚举该代次、入口直接 `AlreadyTerminal`；thinking 已先标 `done` 时连 `had_dangling` 信号也丢失，旧 ACP session/未完成消息永久失去清理机会（RED：`independent-cleanup-probe-red.txt`，exit 101）。

### 修复
- 新增 `IConversationRepository::finalize_exact_cancelled_turn_generation_with_artifact_cleanup`（默认 fail-closed）：精确代次 CAS、receipt 结算、消息终态与 ACP clear 在**同一 SQLite 事务**提交。
- `SqliteConversationRepository` 把 exact-finalize 主体抽为事务内 helper；只在 `Committed/AlreadyApplied` 应用清理，`Stale` 不触碰任何消息/session；清理错误映射为 `Conflict` 并回滚整笔事务。
- `sqlite_acp_session::clear_session_id_in_tx` 复用原 RMW 语义；`clear_session_id` 改为单事务。
- `boot.rs` 只读准备清理写入（读取失败延迟结算）；`service.rs` 不再在结算后单独 best-effort 清理。
- 未新增 migration/表/后台流程/第二套恢复状态；未改 public HTTP 契约。

### 证据
- `followup-cleanup-red.txt`：ACP clear 失败与 thinking 清理失败均错误返回 `Reconciled`，exit 101。
- `followup-cleanup-green.txt`：2 passed；`followup-boot-reconcile.txt`：**10 passed**（新增 ACP、thinking、stranded work 三组失败回滚/重试/幂等用例，保留 remote/未注册 gateway/过期/无证明/健康 session 断言）。
- `followup-app-proof.txt`：真实 host provider/registry **10 passed**。
- `followup-db-conversation-all.txt`：**47 targets / 1673 passed / 0 failed / 0 ignored**（1670→1673，+3 为上述清理故障用例）；Conversation lib 648、DB lib 458、schema 23 不变。
- 宿主 voice 92 / 3 ignored；recorder 32；app boot filter 82；`cargo check -p Flowy` exit 0；`git diff --check` 与改动文件 rustfmt exit 0。
- 编码：移除 `boot_terminal_proof.rs`/`boot_reconcile_sqlite.rs` BOM，恢复顶部破折号，修正新注释 `--?`。

### 证据边界
`router/state.rs` 的 `include_str` 断言只证明源码接线顺序（AppServices 不再早期结算、proof provider 先于 sweep），**不是完整 AppServices→router 行为集成**。行为证据来自真实 SQLite 的 proof-aware 恢复/回滚测试与真实 host proof provider/registry 测试；未完整启动 `build_module_states`/router 全链路。T4 真实 DOM/原生/设备与 AEC 仍未验。


<a id="independent-review-repair-2026-10-25"></a>

## 2026-10-25 独立审查 R1/R2/R3 修复（最新，优先于下方）
独立审查 `independent-review-2026-10-25.zh.md` 确认：T1/T3 的测试数量属实，但 R1 运行代次隔离存在已复现生产缺陷，R2 未走生产 `start_call` 恢复入口，R3 反例绑定了错误列。本节记录修复后的真实状态；下方 2026-10-25 接续记录保留为历史，其中“boot.rs 仍早于证明扫描”的提请已被本节取代。

### R1 启动结算统一到 proof-aware 恢复
- **根因**：`crates/backend/nomifun-app/src/services.rs` 在构造 `AppServices` 时调用 `reconcile_running_conversations_on_boot`。该扫描对所有持久 `running` 行直接 `finalize_orphaned_turn`：关闭 accepted 回执、清空 active operation、推进 epoch，且不要求终止证明；随后 `router/state.rs` 的证明扫描只枚举仍 `running` 或仍有 active operation 的行，提前结算使隔离对象消失。真实 SQLite 反例得到 `status=finished / receipt=completed / active=None`（RED：`independent-remote-probe-matched-red.txt`，exit 101）。
- **最小修复**：删除 `AppServices` 构造期的无证明结算；`boot.rs` 改为仅提供 `settle_proven_interrupted_turn_artifacts`（关闭悬空 thinking/work 消息，仅在确实悬空时清 ACP session）。`ConversationService::try_finalize_proven_orphan_generation` 在持久证明匹配且精确 CAS 成功后调用它，`Stale` 分支不触碰任何消息/session。未新增第二套恢复路径，未改数据库约束。
- **证明 seam 行为**：无 proof provider、remote、未注册 gateway、registry 解析失败/未完整处理、过期/不匹配代次均保持 status、epoch、active operation、accepted receipt、ACP session 与消息原样；有效证明后只结算精确旧 epoch/operation，并保留 `interrupted_by_restart` 结构化回执和消息终态。
- **正式测试**：
  - `cargo test -p nomifun-conversation --test boot_reconcile_sqlite` → **7 passed**（`r1-boot-reconcile.txt`）：nomi/acp/nanobot 有效本地证明、remote 不咨询本地 provider、未注册 gateway、registry 失败、过期编码、泄漏 running 标记保留 session、已终态幂等。
  - `nomifun-app` 的 `boot_terminal_proof` 真实 host 用例 → **10 passed**（`r1-app-proof.txt`）：真实 `BootTerminalProofProvider` + `reap_orphan_agent_processes`；空 registry 证明并结算、不可读 registry 隔离、live legacy entry 保留、自启动 gateway 身份验证后结算、过期快照隔离、remote 隔离。
  - `router/state.rs` 结构测试 → **1 passed**（先 RED `r1-structural-red.txt` exit 101，后 GREEN `r1-structural-green.txt`）：证明 provider 必须先于扫描安装，`services.rs` 不得出现无证明结算。

### R2 经生产 start_call 补完整缓存恢复
- `durable_voice_identity_survives_start_cache_eviction_and_file_reopen` 改为真实 `ConversationVoiceDialogueRecorder`、真实 manager 与文件 SQLite/journal。先用生产 `start_call` 以 `req-cache-recover` + 冻结 `NewVoiceConversation` target 创建；再用更新的回执把 128 上限挤出该回执，关闭数据库与 journal，重建 manager/service/recorder 后从同一 `start_call` 入口重试。
- 断言：同 Conversation、不重复创建、不跨工作区、已接受消息不丢不重、discarded 请求返回 `conversation_already_discarded` 不复活、session_mode 变化返回 `voice_start_request_conflict`、journal 已完成回放 `Cached`、未回答 claim 返回 `GenerationUnknown`。daemon wake 与 model 为注释明确的 IPC/模型模拟边界，路由/仲裁/缓存/存储未 mock。
- 证据：`r2-start-call.txt` **1 passed / 0 failed**；并入宿主 voice 运行 **92 passed / 3 ignored**（`t3-app-voice-lib.txt`）。

### R3 Schema 反例绑定
- `id_schema_contract.rs` 的非法 `linked_device_id` 用例改为“非法 device + 合法 owner”；第二例为“合法 device + 非法 owner（UUIDv4）”，非法值不再固定落在 owner 列。迁移、生产 CHECK、121 表数量与其他契约均未改。
- 证据：`r3-schema.txt` **23 passed / 0 failed**；并入 DB lib **458 passed**。

### 回归与范围
| 检查 | 结果 |
| --- | --- |
| `cargo test -p nomifun-db -p nomifun-conversation --no-fail-fast` | exit 0；47 targets；**1670 passed / 0 failed / 0 ignored**（较 1666 的 +4 来自 boot_reconcile 3→7）；`t6-db-conversation-all.txt` |
| `cargo test -p nomifun-app --lib voice_ --no-default-features` | **92 passed / 0 failed / 3 ignored**；`t3-app-voice-lib.txt` |
| `cargo test -p nomifun-app --test voice_dialogue_integration --no-default-features` | **32 passed / 0 failed**；`t4-voice-dialogue-integration.txt` |
| `cargo test -p nomifun-app --lib boot --no-default-features` | exit 0；**82 passed**，含新增 proof-aware 用例；`t5-app-boot.txt` |
| `cargo check -p Flowy` | exit 0；`cargo-check-Flowy.txt` |
| `git diff --check` / 改动 Rust 文件 `rustfmt --check` | 均 exit 0；仓库 `disable_all_formatting=true`，不声称全仓格式化；`rustfmt-check.txt` |
| 未重跑 | 未改 UI，未重跑 TS/CSS/打包；T4 真实 DOM/原生/DPI/桌宠姿态/物理声学仍未签核，AEC/自然插话本轮非目标 |

修改文件：`crates/backend/nomifun-conversation/src/{boot,lib,service}.rs`、`crates/backend/nomifun-conversation/tests/boot_reconcile_sqlite.rs`、`crates/backend/nomifun-app/src/{services.rs,voice_companion_manager.rs}`、`crates/backend/nomifun-app/src/router/{state,boot_terminal_proof}.rs`、`crates/backend/nomifun-db/tests/id_schema_contract.rs` 及本文档。未提交、未推送。


# rev3 独立复审修复与验证事实

<a id="next-validation-execution"></a>

<a id="continuation-2026-10-25"></a>

## 2026-10-25 接续收口：boot 恢复、schema 契约与持久身份链路

本节优先于 2026-10-24 节；下方原文完整保留为历史。基准仍为 `feat/voice-next` / `6663b1ad2`，接手前 18 个已跟踪修改已导出为 `build.noindex/voice-workspace-display-review/handoff-baseline-20261024-carried.patch`（任务书为未跟踪文件，按状态/哈希保护），未提交、未推送、未切分支、未 rebase。

### 关闭的真实缺陷与修复

1. **启动恢复从未真正结算 running 行（生产缺陷）**
   - 原因：`crates/backend/nomifun-conversation/src/boot.rs` 通过通用 `ConversationRowUpdate { status: Some("finished") }` 更新生命周期；`sqlite_conversation::update` 现已明确拒绝任何通用状态写入（`Generic Conversation updates cannot change lifecycle status`），因此每次启动只打印 warning，`running` 行、悬空 thinking、ACP session 与 accepted 回执都保持原状。
   - 修复：改用仓储的原子 `finalize_orphaned_turn`。同一事务把该 owner/conversation 的 accepted `turn` 回执标记为 completed/failed、清空 `active_turn_operation_id`、推进 `admission_epoch`、把状态 CAS 到 `finished`；仅在检测到悬空 thinking/work 消息时才清 ACP session。`TurnLifecycleTransition::Stale` 不计数并留给后续恢复。
   - 测试夹具不再手写 running：先真实 `claim_turn_delivery_receipt_and_admit_with_candidate`（accepted 回执 + running + active operation + epoch+1），插入投影后的用户消息与悬空 thinking，再调用真实 boot 入口。断言消息内容与 thinking 终态保留、回执 completed、active 清空、epoch 精确 +1、ACP 仅在真实中断时清空、二次启动幂等；另加“无悬空消息的泄漏 running 标记保留 ACP”反例。
   - 证据：`t1-boot-baseline.txt` **0 passed / 2 failed**（旧夹具在 CHECK/UUID 前失败）；`t1-boot-green-1.txt` **2 passed / 1 failed**（真实协议暴露 thinking 父消息缺失）；`t1-boot-green-2.txt` **3 passed / 0 failed / 0.13s**。

2. **Schema 契约数量落后于已登记的新表（契约漂移）**
   - 新增 4 表来源：`067_skill_selection_and_pins.sql`（提交 `6750a8fad86f0965b21266c5c1c9aa8858a83415`）新增 `skill_selection_event`、`conversation_skill_pins`、`skill_invocation_overrides`；`068_linked_devices.sql`（提交 `5f90aecf43b107966dd629e46bf27972451dcfa1`）新增 `linked_devices`。
   - 逐表核对：四表均为唯一 `id INTEGER PRIMARY KEY AUTOINCREMENT`；`linked_devices.linked_device_id` 为 UUIDv7 业务 ID、`user_id` 为 UUIDv7 逻辑引用；`skill_selection_event.conversation_id`、`conversation_skill_pins.conversation_id` 为 UUIDv7 逻辑引用；`skill_invocation_overrides.skill_id` 与 `conversation_skill_pins.skill_id` 为已登记的 source-qualified catalog key（非 UUID 关系 ID）。四表都已在 `PRODUCT_TABLES` 与逻辑引用契约中登记，`validate_id_schema_contract` 通过，无 schema 缺陷、无需迁移。
   - 修复：保留逐表循环断言，显式数量 `117 → 121`，并新增 `skill_selection_and_link_tables_enforce_their_declared_constraints`，在隔离内存库拒绝非 UUIDv7、重复键、空 skill_id、越界 `allow_model_auto` 等错误输入。
   - 证据：`t2-schema-baseline.txt` **21 passed / 1 failed**；`t2-schema-green-1.txt` **23 passed / 0 failed / 1.87s**。

3. **恢复链路缺少进程级组合证据**
   - 新增 `voice_companion_manager::tests::durable_voice_identity_survives_start_cache_eviction_and_file_reopen`：真实文件 SQLite + 真实 recorder journal + 真实 manager start 缓存。先持久化 retained/closed/mode-frozen 三个创建身份与已接受消息，写入 135 条 start 回执迫使 128 上限淘汰；关闭数据库与 recorder 后重开，重建空缓存；断言 retained 身份仍复用同一 conversation、消息仍为 1 条且内容不变，closed 请求返回 `conversation_already_discarded` 且保持 discarded，mode 改变冲突，mode 原值不变；journal 中已完成回放为 `Cached`，未回答 claim 为 `GenerationUnknown`，且不重复插入已接受用户消息。daemon/model 边界没有启动，测试内注释明确为模拟边界；被验证的存储、缓存淘汰、回执与仲裁均为真实实现。
   - 证据：`t3-manager-combined-green-1.txt` **1 passed / 0 failed**；并入最终宿主 voice 运行后 **92 passed / 0 failed / 3 ignored（72.41s）**。

### 最终回归回执

| 命令 | 结果 |
| --- | --- |
| `cargo test -p nomifun-db -p nomifun-conversation --no-fail-fast` | **exit 0**；47 targets，**1666 passed / 0 failed / 0 ignored / 0 filtered**；日志 `t5-db-conversation-all.txt` |
| `cargo test -p nomifun-app --lib voice_ --no-default-features` | **exit 0**；**92 passed / 0 failed / 3 ignored / 427 filtered out，72.41s**；日志 `t5-host-voice.txt` |
| `cargo test -p nomifun-app --test voice_dialogue_integration --no-default-features` | **exit 0**；**32 passed / 0 failed，5.09s**；日志 `t5-recorder-integration.txt` |
| `cargo check -p Flowy` | **exit 0，1m08s**；日志 `t5-desktop-check.txt` |
| `bun test` 21 文件（RESULTS §2.1） | **164 passed / 0 failed / 1240 assertions，7.79s**；`t4-frontend-21files.txt` |
| `bun test` 桌宠 6 文件 | **38 passed / 0 failed / 70 assertions，83ms**；`t4-pet-6files.txt` |
| `bun run typecheck` | **exit 2，61 条既有错误，语音相关 0，与基线一致**；全仓 typecheck 仍非通过；`t5-typecheck.txt` |
| `git diff --check` / 真实 `rustfmt --check`（4 个改动 Rust 文件） | 均 **exit 0**；仓库 `disable_all_formatting=true`，仅为配置内检查通过 |
| `dead-css` / 生产打包 | 本轮无 UI/TS 业务改动，未重跑；既有 dead-css 失败与全仓 TS 基线不通过仍单列，不改基线 |

### 真实交互与原生验证尝试（未签核）

- 使用合法链路启动 `nomifun-web --port 8787 --api-only --insecure-no-auth --data-dir <temp>` 与 `ui` 的 `bun run dev:web`，通过 bsk 连接真实 Chrome 155（extension 0.3.3）导航 `http://127.0.0.1:5173/`。
- SPA 渲染的是 `Flowy - 登录`；`GET /api/auth/status` 返回 `{"success":true,"needs_setup":true,"user_count":1,"is_authenticated":false}`，`GET /api/auth/user` 虽返回安装 owner，但前端受云端登录门禁，无法进入会话列表/语音输入框。没有云端凭据，未绕过鉴权，也没有把登录页当作语音 DOM 证据。
- 截图与日志：`build.noindex/voice-workspace-display-review/t4-live-web/login-blocker.png`、`api.out.log`、`api.err.log`、`ui.out.log`、`ui.err.log`。测试后已按 PID 树终止 API/Vite/本轮 bsk daemon，确认 5173/8787 端口释放并删除临时数据目录。
- 原生桌面、透明首帧、DPI、桌宠 show/hide/owner/位置、真实麦克风/AEC 均未启动验证，保持未签核；静态 164 + 38 项只证明组件/纯逻辑，不替代真实点击或原生画面。

### 工作区与边界

- 代码/测试改动仅 4 个文件：`crates/backend/nomifun-conversation/src/boot.rs`、`crates/backend/nomifun-conversation/tests/boot_reconcile_sqlite.rs`、`crates/backend/nomifun-db/tests/id_schema_contract.rs`、`crates/backend/nomifun-app/src/voice_companion_manager.rs`（仅新增测试）。文档随本记录更新；未改迁移、鉴权、权限策略、ASR/TTS/AEC、harness 或依赖。
- 未提交、未推送、未切分支、未 rebase。`boot.rs` 现在以 `finalize_orphaned_turn` 做启动级结算，仍早于 `router/state.rs` 的终端证明扫描；这延续 boot 入口的原有“启动时统一结算 running”语义，但 external Agent 的证明协议是否应成为唯一权威，建议独立审查后决定是否收窄 boot 范围。

---

## 2026-10-24 接续验证的执行事实（历史）

本节优先于下文提交前/原修复快照。接续基准为 `6663b1ad2`，代码修复基准 `8f62353ea`；本轮修改尚未提交、未推送。

### 已确认并修复的原因

1. **P0 技能事件契约与失败收尾**：生产者从 `6750a8fad8` 起携带 `invocation: "force"`，前端确实消费该字段，不能简单删除。SQLite 批量持久化仍要求不含该字段的精确 JSON，因而拒绝合法事件；失败分支又持有 preparation guard 调用会重新获取相同 gate 的 finalizer。现已对齐新写入契约，保留原不可变 snapshot 字段校验；失败分支在 finalizer 前释放自己持有的 gate/lease，不放松 durable receipt 的终态规则。
2. **P1 迟到主窗口上下文**：`view_only` 的新序号只更新 view，旧完整报告仍可覆盖候选工作区。现在同一 view 序号同时屏蔽旧完整报告，view 与目标更新在同步锁作用域内有序执行。
3. **P1 普通会话 extra 回归**：完整 DB integration 的 `update_extra_replaces_json` 复现普通会话 JSON 被重新序列化；不是只把测试改成忽略顺序。现仅对持有语音生命周期/创建身份的行重写保留字段，普通行恢复原字节替换合同。
4. **扩大回归的根消息夹具**：ACP 历史和 tool retry 测试预置了 `system` 根消息，现有 `ensure_turn_root_message_inner` 会明确拒绝该类型（该校验 `git blame` 指向 `b956168772`，不是本轮修改）。夹具改用生产相同的 hidden/work/text 根消息；保留 artifact、重试身份和历史终态原断言，不放松生产校验。ACP 3 项已转绿，tool retry 最终定向复跑 **2 passed / 0 failed，0.09s**。

### 红灯与证据

- 当前原始大输入测试 45 秒无终态；缩减的单技能 load-only 测试 8 秒亦超时。外层 watchdog 已确认自己创建的测试子进程退出，不依赖 Tokio timeout 处理外部卡住。
- 精确导出 `4aca4cd11`，强制重编译旧提交的三个变化 crate 后，三项测试分别 8 秒超时且不含当前诊断日志，证明既有问题。首次共享缓存误复用当前二进制的对照已作废，不用于归因；归档时间不能作为已重编译的证据。
- 新增 DB 生产事件契约测试修复前 0 passed / 1 failed（0.04s）；新增真实 SQLite 写入失败测试修复前 2 秒 timeout。修复后完整 Conversation lib **648 passed、0 filtered**，原卡住三项和真实失败收尾均通过（15.38s）。
- 主窗口迟到完整报告测试修复前得到 `stale-project`，预期 `project-a`，0 passed / 1 failed；修复后宿主 voice **91 passed、3 ignored**。

### 本轮回归与仍待签核

| 项目 | 实际执行状态 |
| --- | --- |
| Conversation lib | **648 passed**；无 skip、无 filtered；完整库测试原卡住项全部得到真实终态 |
| DB 库/仓储/备份 | DB lib **458 passed**；conversation_repository **91 passed**，含原 JSON 字节替换与新增 skill 契约；backup_bundle **16 passed，128.30s**。没有将备份运行超过 60 秒误判为死锁 |
| 两 crate 扩大回归 | `cargo test -p nomifun-db -p nomifun-conversation --no-fail-fast` **exit 101，1660 passed / 4 failed，3 个失败 target**；其中 tool retry 随后修正同类根消息夹具并定向 **2/2** 通过。当前剩余 boot 两项与 schema 一项，未重新运行整个扩大命令，不能称其 exit 0 |
| 生命周期 integration | 15 passed；含真实有效模型的键盘 send/关闭竞争、bound task、文件 SQLite 重开、130 个后续身份、keyset total/search；130 个创建不等同完整 manager 缓存驱逐演练 |
| 宿主 voice | 91 passed、3 ignored；旧 stop、仲裁、FIFO 与审批仍通过 |
| recorder integration | **32 passed / 0 failed / 0 ignored，5.22s**；有效 preset 与普通创建的权威解析一致、修改 preset 后重试保持快照、多工作区 registry 并发；真实文件数据库关闭并重开，同时重建 service/recorder 后仍正确清理 ended 空占位 |
| 前端 | 164 passed / 1240 assertions；仍是组件静态渲染/适配器证据，不冒称 DOM 点击 |
| TypeScript | 61 条逐条对照，新增 0、移除 0；全仓 typecheck 仍不是通过 |
| dead-css | 仍失败，未动文件与既有 baseline 问题单列；本轮没有 UI 业务修改，也没有扩大棘轮基线 |
| 桌面编译 | `cargo check -p Flowy` **exit 0，3m39s**；不是原生启动回执。UI 源码未改，不机械重复上一轮生产打包 |
| 格式/差异 | `git diff --check` exit 0。直接真实 toolchain `rustfmt.exe --check --edition 2024 --config-path rustfmt.toml` 检查本轮 9 个 Rust 文件 exit 0；仓库 `disable_all_formatting = true`，这是配置执行通过，不证明全仓重新格式化 |
| 桌宠纯逻辑/结构 | **38 passed / 0 failed / 70 assertions，6 文件**；覆盖临时语音 presence、owner、隐藏偏好、投递去重与窗口几何/负坐标。源码断言和纯计算不等同原生 show/透明/DPI 实测。Windows 沙箱内 Bun 无法正确访问目录/别名，正常本机权限下定向命令通过；第一次无 `./` 的过滤命令误扫基线副本，作废，不计证据 |
| P2/P3 | 当前界面能力原生应用入口未启用，应用 inventory 为空，浏览器无 Flowy 页面；不绕过此前本地文件加载限制。没有 DOM 交互、原生透明/DPI/桌宠实际姿态回执，保持 pending |

### 知识与工作区状态

- 代码/测试：`changed-and-verified`（最终联合回执另列）；本轮不修改 migration、权限、ASR/TTS 或 harness。
- 运行态：`pending`；未宣称部署或原生验收通过。
- 文档：以本节为现役事实，下面原文完整保留。
- 规则：`verified-current`；未改 AGENTS/CLAUDE、hooks 或格式策略；仓库 `rustfmt.toml` 当前明确禁用全量格式化，不借本轮改写历史大型文件。
- 记忆：`generated-read-only`；未写入或整理用户长期记忆。
- 工作区：修改仅在本轮 Rust 与文档范围；Git 保持原分支，未提交、未推送。精确基线导出、临时 runner 和 RED/GREEN 日志保留在忽略的 debug 目录供复核，没有清场删除。

日志根目录为 `build.noindex/voice-workspace-display-review/`：`p0-baseline-verified-*`（有效基线）、`p0-contract-red.txt`、`p0-cleanup-red.txt`、`p0-conversation-green.txt`、`p1-view-order-red.txt`、`p1-host-green.txt`、`p1-lifecycle-green.txt`、`p1-recorder-green.txt`、`p4-db-conversation-final.txt`、`p4-typecheck-compare.json`。这些是本机辅助证据，正式可复跑测试保留在仓库。

最终增强回执使用 `p1-recorder-final.txt`、`p3-pet-logical-complete.txt`、`p4-desktop-check.txt`、`p4-tool-retry-green.txt`；扩大回归最终收集使用 `p4-db-conversation-all-targets.txt`。前面的失败/缓存误复用日志不能替代最终回执。

环境异常：首次通过 Cargo 调用格式检查时，Cargo 从 PATH 选择了 mise 的 `cargo-fmt` shim，出现递归子进程。已核实根 PID/完整命令后终止本轮树，并再次查询本轮精确命令匹配数为 **0**；没有终止其他开发进程。清理日志为 `p4-format-owned-cleanup.txt`，之后真实 rustfmt 的回执为 `p4-format-direct.txt`。不把被终止的 Cargo 命令记为通过，不修改全局工具配置。

### 尚未关闭的项目与接续方式

1. **扩大回归**：`boot_reconcile_sqlite` 两项在初始化前因 `OWNER = "user_0190..."` 被 canonical UUIDv7 parser 拒绝，当前没有改该夹具/boot 业务。它还直接通过通用 update 写入 running；接续应先按当前 claim/admission 合同重建真实 crash fixture，再判断 boot reconcile 行为，不仅移除字符串前缀就宣称通过。`id_schema_contract::every_product_table_has_one_integer_autoincrement_row_primary_key` 断言 117 张表，实际 121；本轮未改 schema/migration，也未将预期数直接调大。接续应列出增加的 4 张表和提交来源，核对每表约束及登记后再更新契约。不是原 P0 三项卡住，不能混为一类。完整两个 crate 的 all-target 命令保持真实失败状态，不删测、不 filtered。
2. **P1 剩余证据强度**：130 个持久身份重开与现有 manager 128 项缓存淘汰单测分别通过，但没有把真实 daemon、manager 缓存驱逐和文件数据库恢复串成同一个进程级演练。多工作区并发解析/既有迟到发布测试也不是实际两个窗口操作录像；接续不把这些证据合并称“原生链路已测”。
3. **P2/P3**：需使用明确启用的 DOM/原生应用 runner，复用原计划 §4/5 取得 More 键盘焦点、布局 bounding box、双主题/双语、原生 show/位置/资源 ready 时序与 DPI 证据；当前不绕过工具拒绝，不要求用户反复说话代跑逻辑测试。真实 AEC/自然插话仍不在本轮范围。
4. **交付**：代码/测试和文档在当前 `feat/voice-next` 工作树，未提交、未推送。TS/dead-css 仍按独立失败记录，不通过改基线使门禁变绿。下方原修复快照不作为当前进度。

可复跑主命令（Windows 使用可用的真实 Cargo/Bun 二进制，避免失效 shim）：

```powershell
cargo test -p nomifun-conversation --lib
cargo test -p nomifun-conversation --test voice_placeholder_lifecycle
cargo test -p nomifun-app --lib voice_ --no-default-features
cargo test -p nomifun-app --test voice_dialogue_integration --no-default-features
cargo test -p nomifun-db -p nomifun-conversation --no-fail-fast
cargo check -p Flowy
# 从 ui/ 执行；必须使用明确的 ./ 文件路径，防止 Bun 递归过滤匹配 debug 基线副本。
bun test ./src/renderer/hooks/useCompanionWindowsSync.test.ts ./src/renderer/pages/companion/windowGeometry.test.ts ./src/renderer/pages/companion/voicePetDelivery.test.ts ./src/renderer/pages/companion/eventScope.test.ts ./src/renderer/pages/companion/companionPresence.test.ts ./src/renderer/pages/companion/companionScreenClamp.test.ts
```

---

> **本地提交回执（2026-10-24）**：本轮代码、测试及原文档已提交为 `8f62353ea386d483fa26f911b62000191d9e3d5a`；署名门禁通过，未推送。后续按 [下一步验证修复计划](NEXT-VALIDATION-PLAN-2026-10-24.zh.md) 的 P0–P4 执行，该计划尚未运行。以下“未提交/无提交授权”为代码提交前快照，保留作历史，不代表当前 Git 状态。

> 2026-10-24；分支 `feat/voice-next`；接管基准 `4aca4cd11333a0d4151d03fb8941186c1017462b`。修复与原实施改动均在工作树，未提交、未推送。本文覆盖本目录历史“全部通过”声明；旧报告原文保留作历史参考，不能用于当前签核。

## 1. 本轮结论与边界

已修复独立审查发现的创建失败、生命周期竞态、清理未接线、持久身份冲突、迟到快照复活活动行、状态误报和侧栏交互缺口。进一步补齐首页模型/预设冻结、有效输入后发布自动复用指针，以及晚到回复经过另一条 SQL 投影路径时必须保留会话的遗漏。

本轮复用 Conversation、现有 SQLite extra、启动仲裁、journal 和恢复 tick；没有新增 migration、权限模式、依赖或 agent harness。正式消息仍走共享 Conversation 历史。未使用占位采用逻辑关闭，保留 creation key 作为终态身份，不能通过公开 PATCH 重新打开。

**不能签核“24 项全部完成”**：组件静态渲染与源码检查不是浏览器点击证据，代码编译不是原生透明首帧或物理声学证明。完整 Conversation 回归有三项技能加载测试未结束，另有全仓 TypeScript/dead-css 门禁失败，详见第 4 节。

## 2. 修复映射

| 审查项 | 修复与最小合同 |
| --- | --- |
| F1 项目创建触发公开字段保护 | 公开创建仍拒绝保留字段；内部 `create_inner` 在校验之后注入可信 workspace 与创建身份。显式新建对默认、项目均分配独立 request 身份。 |
| F2 关闭与写入竞争 | SQLite 同一写事务中检查终态、保留生命周期并插入消息；关闭条件在事务中重新检查消息、active turn 与 accepted receipts。补齐 dialogue pair 的独立原生 SQL 写入路径。 |
| F3 清理为不可达接口 | 删除无调用方的 `/session/discard_empty`、handler 与前端 helper。宿主恢复 tick 根据可信 terminal、matching session/request、主窗口页面和任务保护集合执行 ended/cleanup。 |
| F4 读回执保留空占位、改名不保留 | 读取不再改变生命周期；用户编辑在成功写入事务中保留。失败写入不制造 retained。 |
| F5 公开 extra 可覆盖终态 | 生命周期/创建身份为后端保留字段；普通 repo 更新保留不可变身份和 retained 状态；clone 移除实例生命周期身份。 |
| F6 128 项缓存不足以保证幂等 | creation key 按 user + request 持久化；重试比对 canonical workspace、requested mode、model、preset；ended/discarded 请求不能重建。owner 从已有 Conversation 读取。 |
| F7 多订阅迟到 seed 复活活动行 | 共享单调快照消费者维护活动归属与 retired instance；慢订阅也交付共享最新快照。pending request 仍按精确身份结算。 |
| F8 null/断线/终态误报倾听 | null 显示 syncing；离线显示不可用；结束态显示未通话；无声学 readiness 时显示文字继续。 |
| F9 More 不可聚焦、开菜单重挂载 | 保留同一根节点；opacity 控制可见性而非 display:none；focus-within、Enter/Space、ContextMenu/Shift-F10、Escape 恢复焦点；语音行 hovercard 与操作互斥。 |
| F10 活动通话仍可删除 | 行菜单禁用、共享批量删除入口校验；Conversation 服务安装宿主弱引用 guard，保护统一删除路径。挂断与取消任务保持独立。 |
| F11 仍有麦克风/跑马灯/错误 aria | 所有语音类型采用被动 AudioLines；活动连接点与动效持续保留；语音标题 ellipsis，active/idle 中英可访问文案分开；减少动效时静止。 |
| F12 反例仅源码字符串断言 | 正式加入真实 SQLite 生命周期与 recorder 集成回归；前端改为真实组件静态渲染和异步订阅反例。浏览器交互探针已编写，但没有执行成功，不能作为通过证据。 |

## 3. 接线与数据事实

### 3.1 新建、冻结与保留

首页发送 `new_voice_conversation`，冻结 workspace、模型引用、preset 引用和显式模式。preset 继续由既有 Conversation 创建服务权威解析；不接收客户端 snapshot，不静默丢弃失效 preset，不能将非 Nomi 预设切入该语音执行入口。快聊模型选择机制不在此次改变范围内。

`provisional → retained` 的原子保留点是有效消息/接纳写入或成功的用户编辑；不是读取回执，也不等待 LLM 成功。`ignore` 仍不产生正式问答。显式占位尚未保留时不会替换既有自动目标；保留后将默认/项目指针发布至既有 workspace registry。默认槽使用空字符串内部键，合法 canonical 项目绝不会使用该键；项目 30 分钟规则及默认工作区既有复用规则保持不变。

显式目标解析与迟到投影发布指针共用既有 journal 锁，串行保护 registry 的读取、修改和原子文件替换，避免另一工作区的 map 更新被覆盖。不是另建一份复用状态。

### 3.2 结束与离页

现有 1 秒恢复 tick 只在 connected 且身份可核实时处理终态。`ended` 的持久标记与空占位关闭分开：后台任务未结束也可记录通话已结束，但任务、reservation、当前活动 binding、未投影 journal 输入和仍选中的页面阻止清理。

桌面主窗口通过 `report_voice_context(..., view_only)` 上报“正在看哪个页面”，与自动语音目标候选分开；进入设置可记录已离页，但不清空此前选择的工作区。没有可信 view 上报则暂不清理。断线/unknown/崩溃本身不证明结束，不按短超时猜测删除。

### 3.3 UI

首页 icon-only；输入框左上角胶囊承载阶段；侧栏左声波只表示类型/活动归属，右 More 固定槽承载操作。普通胶囊没有空详情，审批、逐项队列取消、unknown 核对和历史重试保留。CSS 节奏动效不是实际音量或全双工承诺。当前生产 AEC 未在本轮接入。

## 4. 本轮验证

原始日志位于忽略目录 `build.noindex/voice-workspace-display-review/`，不是可移植的签核附件。以下正式测试文件随代码保留，可复跑。

| 检查 | 当前证据 |
| --- | --- |
| `cargo test -p nomifun-conversation --test voice_placeholder_lifecycle` | **11 passed**，含 24 次真实 close/append 竞争、改名、公开保留字段、ended 旧请求、共享删除 guard。无模型网络调用。 |
| `cargo test -p nomifun-app --test voice_dialogue_integration --no-default-features` | **30 passed**；真实 service/SQLite/recorder，新增终态/离页/重建 recorder 恢复、迟到 pair、首页模型冻结与 preset 不静默忽略、默认/项目指针发布。 |
| `cargo test -p nomifun-app --lib voice_ --no-default-features` | **90 passed、3 ignored**；忽略项不计通过。 |
| 前端既有定向命令（21 文件，含 `rev3Probes.test.tsx`） | **164 passed、0 failed、1240 assertions**；静态组件渲染、适配器异步事件/请求验证。不是 164 项 DOM 点击。 |
| `cargo check -p Flowy` | 最终 DTO/宿主/桌面桥接增量编译通过，exit 0，2m03s；未启动真实桌面窗口。 |
| `bun run build:ui` | 最终状态/共享快照改动后生产打包通过，exit 0。 |
| TypeScript | 最终直接 `tsc --noEmit` 为 61 条；与修复前诊断集合差分新增 0、移除 0（统一工作目录路径前缀后比较）。不能把全仓 typecheck 记为通过。 |
| i18n/theme/icons/button-layout/motion/css-scope/架构边界 | 本轮定向脚本已通过；i18n 11764 keys。 |
| dead-css | **失败**；报告指向本轮未改动的 codeBlock、Wallpaper、HerdsmanProviderNotice、CloudLogin、技能消息、eval、视频等文件及旧基线条目；没有修改基线掩盖失败。 |
| DB/Conversation lib 回归 | DB **458 passed**。Conversation 完整运行中共 3 项技能加载测试长期不结束，已终止该次测试进程；单独重跑 `send_message_allows_large_explicit_skill_snapshot` 45 秒仍超时并清理 owned child。显式跳过这 3 项后 **644 passed、3 filtered out**；过滤不是通过。尚未证明卡住与本轮无关，不能标成“已知基线通过”。 |
| 浏览器/原生 | 浏览器交互探针已编译，但运行未得到有效结果；工具拒绝本地文件加载后停止，不绕过策略。原生透明/DPI/首帧、真实声音仍未签核。 |

正式测试入口：

- `crates/backend/nomifun-conversation/tests/voice_placeholder_lifecycle.rs`
- `crates/backend/nomifun-app/tests/voice_dialogue_integration.rs`
- `ui/src/renderer/pages/voiceSession/rev3Probes.test.tsx`
- 未执行成功的交互探针：`ui/src/renderer/pages/voiceSession/tests/voiceComposerRev3.browser-probe.tsx`（不被生产导入）。

## 5. 24 项验收的准确解释

| 用例 | 当前可证明范围 / 剩余 |
| --- | --- |
| LC-01/02 | 默认和项目独立创建、相同身份复用、配置改变拒绝有真实服务证据；没有首页两次点击的浏览器录像。 |
| LC-03/04 | 30 项 recorder 集成及 90 项宿主测试覆盖路由冻结/单通话/unknown；本轮未操作真实 KWS。 |
| LC-05 | 冻结目标和现有跨会话隔离测试通过；真实迟到 ASR 设备未测。 |
| LC-06/07 | 终态、离页保护、重建 recorder、晚到消息、改名保留有 SQLite 测试；宿主 tick/主窗口 view 上报接线已编译。 |
| LC-08 | 既有 ignore/FIFO/审批恢复回归保持；无声学硬件结论。 |
| LC-09 | 原子 close/append 竞争和迟到 pair 有实测；不夸大为所有 send/task 组合压力测试。 |
| LC-10/11 | 持久终态/公开保护、旧会话不清理、指针发布有测试；未做整个进程断电恢复演练及所有分页组合穷举。 |
| LC-12 | 显式模型冻结、变化冲突和失效 preset 不丢弃有真实集成测试；有效 preset 完整目录解析沿既有服务，本轮没有另建全套预设 fixture。 |
| UI-01/04/05/07/09 | 组件渲染/异步快照/代码边界证据；不等于所有虚拟列表/入口的实际点击验收。 |
| UI-02/03/08/11 | 布局、焦点与 reduced-motion 代码已修；中英静态渲染通过。浏览器宽度/hover/焦点、明暗视觉与 DPI 尚未签核。 |
| UI-06 | 共享服务删除防线及宿主回归通过；挂断与取消任务独立沿用既有测试。 |
| UI-10 | 审批/队列/unknown/历史重试原有定向组件及适配器回归通过，未取得浏览器交互回执。 |
| UI-12 | 沿用既有桌宠 owner/耳机/临时显示逻辑；原生窗口与桌宠姿态未签核，不重复声明通过。 |

## 6. 接续执行顺序

1. 先阅读本文及当前 diff；不得根据历史 RESULTS 的勾选重新宣布全绿。
2. 保留语音定向测试的 green 状态，补查 3 项技能加载测试卡住的具体调用边界，并区分 HEAD 基线与修复回归。不要为绕过测试修改业务接纳合同。
3. 在允许的交互验证环境执行已有探针：More 的真实 click/keyboard/Escape、菜单互斥、固定槽宽度、中英/明暗/reduced-motion。没有实际回执的项目保持未签核。
4. 原生验证仅对透明首帧、正确 Bolt/owner、保存位置和 DPI 作验收；不以 Web 测试替代。声学能力未变，不宣称接入 AEC。
5. 当前没有提交/推送授权；本轮不修改 Git 历史或分支。

## 附录：历史证据

同目录 `PROGRESS.md`、`RESULTS.zh.md`、`BLOCKED.md` 的旧正文完整保留。其“160 项”“24 项全部完成”“discard_empty 已调用”“全部质量门禁通过”等属于原执行报告，已被本文逐项纠正，不是本轮复审结论。
