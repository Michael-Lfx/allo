> **2026-10-25 宿主启动行为验收更新**：已补齐真实文件库、OS 服务锁和 `AppServices → create_router` 组合启动测试；旧构造期提前结算的临时退化触发 3/3 行为失败，源码逐字恢复后 boot 84 项通过。此前“仅有源码接线证据”的对应缺口已关闭，原生/设备与二进制冷启动仍单列；本节优先于下方历史描述，提交状态以 Git 为准。详见 [宿主启动验证记录](HOST-BOOT-VALIDATION-2026-10-25.zh.md).

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


## 2026-10-25 独立复审 follow-up：恢复清理原子化
- **问题**：首轮在生命周期 CAS 提交 `finished/active=None` 后才 best-effort 清理。ACP clear 失败/两步间崩溃后，boot 不再枚举该代次，入口直接 `AlreadyTerminal`；thinking 先标 done 后失败还会丢失 `had_dangling` 信号。独立反例 RED：`independent-cleanup-probe-red.txt` exit 101。
- **修复**：新增 DB 契约 `finalize_exact_cancelled_turn_generation_with_artifact_cleanup`，把精确旧 epoch/operation 的 CAS、receipt 结算、thinking/work 终态与 ACP clear 放入同一 SQLite 事务；清理失败返回 Conflict 并回滚。`Stale` 不触碰消息/session；未新增 migration/表/后台流程/第二套恢复状态。`boot.rs` 改为只读准备，`service.rs` 删除结算后 best-effort 清理。
- **测试**：新增 ACP clear 失败、thinking 清理失败、stranded work 清理失败三组正式用例；`followup-boot-reconcile.txt` **10 passed**，首次 RED `followup-cleanup-red.txt` exit 101。两 crate all-target **47 targets / 1673 passed / 0 failed / 0 ignored**；宿主 voice **92 / 3 ignored**；recorder **32**；`cargo check -p Flowy` exit 0。
- **证据边界**：`router/state.rs` 的 `include_str` 测试仅为源码结构接线防线，不是完整 AppServices→router 行为集成；行为证据为真实 SQLite 恢复/回滚与真实 host provider/registry 测试。T4 DOM/原生/设备与 AEC 仍未验。
- **编码**：移除 BOM，恢复 `boot_terminal_proof.rs` 顶部破折号，修正 `boot_reconcile_sqlite.rs` 的 `--?` 注释。
- 详见本地证据目录 uild.noindex/voice-workspace-display-review/opencode-repair-20261025/FOLLOWUP-FINAL_REPORT.zh.md 与 [修复记录](REPAIR-VERIFICATION-2026-10-24.zh.md#followup-2026-10-25-cleanup)。未提交、未推送。

## 2026-10-25 独立审查修复（R1/R2/R3）
- **基准与范围**：`feat/voice-next`，HEAD `6663b1ad261b3e997e796cc5ffc75a6e8104c65a`；保留接手前全部未提交成果，未 commit/push/reset/stash/checkout/rebase。
- **R1 统一可信启动恢复（已修复）**：独立审查反例证明 `services.rs` 在构造 `AppServices` 时调用无证明的 boot 扫描，先行 `finalize_orphaned_turn` 清空代次/回执，后续 proof-aware 扫描失去对象（remote 被错误置为 `finished/completed/active=None`）。现移除该构造期结算；`boot.rs` 只保留“已证明代次”的消息/ACP 清理 helper，由 `try_finalize_proven_orphan_generation` 在精确 CAS 成功后调用。无证明时 status/epoch/active operation/accepted receipt/ACP session/消息全部不变；有效证明后按精确旧 epoch/operation 结算并保留中断消息终态与 `INTERRUPTED_BY_RESTART` 结构化结果。
- **R1 新增正式反例/证明**：conversation `boot_reconcile_sqlite` 覆盖 nomi/acp/nanobot 有效本地证明、remote 不咨询本地 provider、未注册 gateway、registry 失败/不完整、过期代次、泄漏 running 标记保留 session、已终态幂等；app `boot_terminal_proof` 使用真实 `BootTerminalProofProvider` + 真实 `reap_orphan_agent_processes` 报告覆盖空 registry 证明、不可读 registry、live legacy entry、自启动 gateway 身份验证、过期快照、remote 隔离；`router/state.rs` 结构测试固定“先装证明 provider，再跑结算”，且 `services.rs` 不得出现无证明结算。RED：`r1-structural-red.txt` exit 101；GREEN：`r1-boot-reconcile.txt` 7 passed、`r1-app-proof.txt` 10 passed、`r1-structural-green.txt` 1 passed。
- **R2 生产 start_call 恢复链路（已修复）**：`durable_voice_identity_survives_start_cache_eviction_and_file_reopen` 现使用真实 `ConversationVoiceDialogueRecorder` + 真实 manager + 文件 SQLite/journal，从生产 `start_call` 以同一 `request_id`/冻结 target 创建；用更新回执挤出 128 缓存、关闭并重开数据库与 journal、重建 manager/service/recorder 后仍从 `start_call` 重试。断言同 Conversation、不重复创建/不丢消息、工作区不漂移、discarded 不复活、mode 变化冲突、journal 回放不重复、unknown 不重发。daemon wake 与 model 为明确标注的 IPC/模型模拟边界；路由、仲裁、缓存、存储均为真实实现。GREEN：`r2-start-call.txt` 1 passed。
- **R3 Schema 反例绑定（已修复）**：`non-UUIDv7 linked_device_id` 现在绑定非法 device + 合法 owner；第二例绑定合法 device + 非法 owner，不再把非法值固定落在 owner 列。迁移、生产 CHECK、121 表数量与其他契约未改。GREEN：`r3-schema.txt` 23 passed。
- **最终回归**：`t6-db-conversation-all.txt` 47 targets / 1670 passed / 0 failed / 0 ignored；Conversation lib 648、DB lib 458、schema 23；`t3-app-voice-lib.txt` 92 passed / 3 ignored；`t4-voice-dialogue-integration.txt` 32 passed；`t5-app-boot.txt` exit 0；`cargo-check-Flowy.txt` exit 0；`git diff --check` 与改动 Rust 文件 `rustfmt --check` exit 0（仓库 `disable_all_formatting=true`，不声称全仓格式化）。
- **边界更正**：上文 2026-10-25 接续记录中“boot.rs 仍早于证明扫描”的提请已被本次修复取代；T4 真实 DOM/原生/设备仍未签核，AEC/自然插话为本轮非目标。未提交、未推送。

# 语音交互优化 rev3 进度与执行记录

## 2026-10-25 接续收口（T0–T5）

- **基准与保护**：`feat/voice-next`，HEAD `6663b1ad261b3e997e796cc5ffc75a6e8104c65a`（ahead 2）。接手前 18 个已跟踪修改已导出为 `build.noindex/voice-workspace-display-review/handoff-baseline-20261024-carried.patch`，另有 `.stat.txt`、`-status.txt`、`-hashes.txt`（任务书为未跟踪文件，按状态/哈希保护）；未 reset/stash/commit/push/切分支/rebase。
- **T1 启动恢复（关闭）**：真实缺陷是 `boot.rs` 通过通用 `ConversationRowUpdate` 写 `status=finished`，而 SQLite 仓储已明确拒绝通用生命周期写入，恢复循环只记 warning，running 行永远不会被结算。现改用仓储的原子 `finalize_orphaned_turn`，同一事务终止 accepted 回执、清空 active operation、推进 admission epoch 并置 finished；仅在存在悬空消息时清 ACP session。测试改为真实 claim/admission/receipt 状态并增加泄漏 running 标记与幂等复跑。RED `0 passed / 2 failed` → GREEN `3 passed / 0 failed / 0.13s`。
- **T2 Schema 契约（关闭）**：实际新增 4 表均来自 `067_skill_selection_and_pins.sql`（提交 `6750a8fad86f0965b21266c5c1c9aa8858a83415`：`skill_selection_event`、`conversation_skill_pins`、`skill_invocation_overrides`）与 `068_linked_devices.sql`（提交 `5f90aecf43b107966dd629e46bf27972451dcfa1`：`linked_devices`）。四表均为 `id INTEGER PRIMARY KEY AUTOINCREMENT`，业务 ID/逻辑引用已在 `PRODUCT_TABLES`、UUIDv7 与逻辑引用契约登记；无 schema 缺陷，故把显式数量 `117 → 121` 并保留全表逐表断言，同时新增隔离内存库的约束反例。RED `21 passed / 1 failed` → GREEN `23 passed / 0 failed`。
- **T3 恢复链路（关闭）**：新增真实文件 SQLite + 真实 recorder journal + 真实 manager 启动缓存组合演练：135 次 start 回执触发 128 上限淘汰，随后重开数据库与 journal、重建空缓存，仍按持久创建身份复用同一会话；已关闭请求返回 `conversation_already_discarded` 且不复活，改变 mode 冲突，已完成 journal 回放 `Cached`，未回答 claim 返回 `GenerationUnknown`，已接受消息仍只有 1 条。daemon/model 边界明确为未启动的模拟边界，存储、缓存淘汰、回执与仲裁均为真实实现。
- **T4 交互与原生（部分关闭，真实 DOM/原生保持阻断）**：静态 21 文件 **164 passed / 0 failed / 1240 assertions**，桌宠逻辑/结构 6 文件 **38 passed / 0 failed / 70 assertions**。用合法 runner 启动 `nomifun-web --api-only --insecure-no-auth` + Vite `webdev` 并通过 bsk 操作真实 Chrome；SPA 仍落在 `Flowy - 登录`（`/api/auth/status` 返回 `needs_setup: true`、`is_authenticated: false`），无云端凭据，未取得语音页面真实 DOM 回执。未绕过鉴权，截图与日志留在 `t4-live-web/login-blocker.png`。原生透明首帧、DPI、桌宠实际显示未启动，保持未签核。
- **T5 回归（通过）**：`cargo test -p nomifun-db -p nomifun-conversation --no-fail-fast` **exit 0，47 targets，1666 passed / 0 failed / 0 ignored / 0 filtered**（含原 2 项 boot 与 schema 项）；`nomifun-app --lib voice_` **92 passed / 0 failed / 3 ignored**；recorder integration **32 passed / 0 failed**；`cargo check -p Flowy` **exit 0（1m08s）**；`git diff --check` exit 0；4 个改动 Rust 文件的真实 `rustfmt --check` exit 0。TypeScript 仍为 **61 条既有错误、语音相关 0、与基线一致**，全仓 typecheck 仍非通过；未改 UI，未重跑生产打包；dead-css 未改基线，仍是既有失败。
- **未关闭项**：真实 DOM 交互（云端登录阻断）、真实原生透明首帧/DPI/桌宠姿态、物理声学/AEC、全仓 TypeScript 与 dead-css 既有问题。未提交、未推送。现役事实见 [2026-10-25 接续收口记录](REPAIR-VERIFICATION-2026-10-24.zh.md#continuation-2026-10-25)。
- **边界提请审查**：`boot.rs` 现以 `finalize_orphaned_turn` 做启动级结算，仍早于 `router/state.rs` 的终端证明（terminal-proof）编排；被 boot 结算的行不会再次进入证明扫描。该行为延续了 boot 入口“崩溃后统一结算 running”的原设计，但不是本轮新增的证明协议改造；外部 Agent（remote/openclaw）证明语义如需成为唯一权威，应在后续独立变更中收窄 boot 范围。

## 接手开工回执 (2026-10-24 接续执行)
- **基准与分支**: HEAD `6663b1ad261b3e997e796cc5ffc75a6e8104c65a`, `feat/voice-next` (ahead 2)。
- **目标**: 依任务书完成 T0–T5，修复 boot 启动恢复 2 项失败、核查解决 schema 契约失败、补齐持久恢复/缓存淘汰链路、验证交互与桌宠联动。
- **顺序**: 严格单 Agent 串行 T0(基准与保护) -> T1(启动恢复修复) -> T2(Schema契约核查) -> T3(恢复链路补全) -> T4(交互与原生验证) -> T5(全量回归与收口)。
- **最大风险**: 启动恢复误绕过真实触发器/破坏未决会话数据隔离、盲目修改 schema 契约数字掩盖真实字段缺陷、跨进程/文件重开时状态不一致。
- **保护措施**: 当前工作区 17 个未提交修改已完整导出到 `build.noindex/voice-workspace-display-review/handoff-baseline-20261024.patch`，不 commit/push/rebase/切分支。

> **接续交接入口**：[Agent 任务书](AGENT-HANDOFF-2026-10-24.zh.md)。按 T0–T5 接手既有未提交成果，先处理 boot/schema 失败，再补恢复与交互证据。本次仅新增交接文档；不代表这些剩余任务已执行。

> **2026-10-24 接续执行（优先）**：P0 原三项技能加载卡住已复现、完成旧提交对照并修复；完整 Conversation lib 648 无过滤通过。P1 增补真实 SQLite 重开、有效 preset、接纳/关闭竞争、registry 并发与迟到上下文序号反例，生命周期 15、recorder 32、宿主 91 通过。前端 164、桌宠纯逻辑/结构 38 通过。P2/P3 真实交互/原生显示尚未签核，扩大回归与门禁失败单列，未提交、未推送；准确命令和结果见 [现役执行记录](REPAIR-VERIFICATION-2026-10-24.zh.md#next-validation-execution)。下文保留历史。

> **2026-10-24 交付与接续回执**：修复代码本地提交 `8f62353ea` 已完成，未推送。新增 [P0–P4 验证修复计划](NEXT-VALIDATION-PLAN-2026-10-24.zh.md)，先定位 3 项技能测试，再补并发/持久恢复和真实交互证据；本次只整理计划，不执行下一阶段。下方未提交描述按提交前历史读取。

> **2026-10-24 独立复审修复事实（优先阅读）**：原下文“Done/全部通过”声明不再作为验收结论。创建、原子保留/关闭、恢复清理、持久请求身份、模型/预设冻结、自动指针、共享快照和侧栏防线已修复；真实生命周期 11、recorder 集成 30、宿主 voice 90、前端 164 项通过。完整 Conversation 有 3 项技能测试卡住；TS/dead-css 与浏览器/原生边界单列。见 [本轮修复与验证](REPAIR-VERIFICATION-2026-10-24.zh.md)。未提交、未推送，以下历史原文保留。

- **基准**: HEAD `4aca4cd11333a0d4151d03fb8941186c1017462b` (feat/voice-next)
- **目标**: 侧栏左声波/右More、输入框左上角状态胶囊、首页icon-only与新建/返回区分、占位生命周期安全关闭
- **顺序**: 严格串行 V0(基准与反例) -> V1(生命周期与仲裁) -> V2(侧栏与More互斥) -> V3(胶囊与控制接替) -> V4(全量验证与门禁)
- **最大风险**: 占位关闭与迟到接纳并发导致数据丢失、首页新建未决状态与活动通话仲裁错乱、More按钮事件冒泡
- **基线采样**: 20文件 157 pass / 0 fail / 1213 assertions (3.29s)；TS 错误精确为 61 条既有基线（语音 0 错误）
- **反例状态**: 首页错误复用、关闭与迟到接纳竞争、More事件冒泡三项反例已在 `ui/src/renderer/pages/voiceSession/rev3Probes.test.tsx` 建立并通过修复转绿。

---

## 阶段执行记录

### V0: 基准验证与反例探针固化 (Done)
- HEAD 核实为 `4aca4cd11333a0d4151d03fb8941186c1017462b`。
- TypeScript 基线准确核实：全仓共 61 条既有错误（集中在 `videoCanvas`, `WallpaperSection`, `MessageText` 等非语音模块），语音相关 0 错误。
- 建立正式隔离探针测试 `ui/src/renderer/pages/voiceSession/rev3Probes.test.tsx`，固化三个反例：
  1. Probe 1: 首页呼叫 Intent target 必须使用 `new_voice_conversation`，杜绝复用旧会话历史。
  2. Probe 2: 占位生命周期契约（`provisional` / `retained` / `discarded`），服务端屏障持久化与过滤。
  3. Probe 3: 会话列表行结构，左侧固定声波槽位，右侧语义 `<button>` More 槽位且阻止冒泡。

### V1: 生命周期与单通话仲裁 (Done)
- **DTO**: 在 `crates/backend/nomifun-api-types/src/voice.rs` 增加 `VoiceCallTarget::NewVoiceConversation { workspace: Option<String> }`。
- **数据库过滤**: 在 `crates/backend/nomifun-db/src/repository/sqlite_conversation.rs` 的 `append_filter_conditions` 与 `search_messages` 中增加 `json_extract(c.extra, '$.voice_lifecycle') != 'discarded'` 过滤，实现 0 migration 的关闭占位隐形。
- **生命周期服务**: 在 `crates/backend/nomifun-conversation/src/service.rs` 实现 `create_provisional_voice_conversation`、`retain_provisional_voice_conversation`、`discard_provisional_voice_conversation`，并在消息添加时若已处于 `discarded` 则安全拒绝，避免已关闭身份被迟到消息复活。
- **App 仲裁与路由**: 在 `crates/backend/nomifun-app/src/voice_dialogue.rs` 与 `voice_companion_manager.rs` 实现对 `NewVoiceConversation` 的接纳，增加 `/api/voice/companion/session/discard_empty` 路由及管理器方法。
- **验证**: `cargo test -p nomifun-app --lib voice_` (90 passed / 0 failed)，`cargo test -p nomifun-conversation --lib voice` (9 passed / 0 failed)。

### V2: 侧栏左声波与右 More 互斥 (Done)
- **Row 布局结构**: 修改 `ui/src/renderer/pages/conversation/SessionList/ConversationRow.tsx`：
  - 声波图标移至标题左侧独立槽位（活跃通话为 `AudioLines`，普通语音为 `Voice`）。
  - 右侧 More 槽位固定为 `right-8px`，去除原右侧声波标记，消除 hover 时由于标记显隐导致的标题宽度跳动。
  - More 改为语义 `<button type="button" aria-haspopup="true">`，点击时调用 `event.stopPropagation()` 阻止冒泡至行导航。
  - 打开下拉菜单时，外层 hovercard Popover 自动抑制（`if (menuVisible) return renderRow()`），杜绝浮层重叠。
- **低频音量隔离**: 在 `ui/src/common/adapter/voiceCompanion.ts` 与 `useVoiceSnapshot.ts` 导出轻量级 `useActiveVoiceConversationId()`，会话列表仅监听活跃 conversation ID 的变化，杜绝高频 PCM 音频包（RMS/peak）引发全列表重渲染。
- **验证**: `ConversationRow.structure.test.ts` (6 passed)、`ConversationRow.voice.test.tsx` (3 passed)、`rev3Probes.test.tsx` (3 passed) 全部通过。

### V3: 输入框胶囊与首页精简 (Done)
- **首页电话按钮**: 修改 `ui/src/renderer/pages/guid/components/GuidActionRow.tsx`，移除原有文字 label，仅保留电话图标，tooltip 与 aria-label 根据 `isVoiceActiveElsewhere` 动态切换为“开始通话”与“返回通话”。
- **输入框状态胶囊**:
  - 在 `ui/src/renderer/components/chat/SendBox/index.tsx` 增加 `statusSlot`，渲染于左上方。
  - 在 `ui/src/renderer/pages/conversation/platforms/nomi/NomiSendBox.tsx` 移除原底部状态行，将 `VoiceComposerStatus` 移入 `statusSlot`。
  - 重构 `ui/src/renderer/pages/voiceSession/VoiceComposerStatus.tsx` 为高度 26px 的紧凑圆角胶囊（12px 文字、14px 图标、`role="status"`），普通状态只读不可点击，任务确认/排队/错误重试/挂断完整接替。
- **双语词条**: 在 `zh-CN/voice.json` 与 `en-US/voice.json` 补齐 `callActiveMarker` 与 `composer.pill.*`，通过 `bun run gen:i18n` 与 `bun run check:i18n` 校验。
- **验证**: `VoiceComposerStatus.test.tsx` (4 passed, 144 断言) 中英双语均通过。

### V4: 全量质量门禁与签核 (Done)
- **前端定向与基线测试 (21 文件)**: 160 pass / 0 fail / 1218 expect() calls (2.84s)，无一失败。
- **Rust 后端语音测试**:
  - `nomifun-app --lib voice_`: 90 passed / 0 failed (55.61s)。
  - `nomifun-conversation --lib voice`: 9 passed / 0 failed (2.75s)。
- **TypeScript 诊断**: 保持全仓 61 条既有基线错误，本次修改净新增 0 条错误。
- **静态质量门禁**:
  - `bun run check:i18n`: PASSED (11762 keys, 55 modules).
  - `bun run check:theme`: PASSED (6 CSS files verified).
  - `bun run check:icons`: PASSED (389 files using `@icon-park/react`).
  - `bun run check:button-layout-contract`: PASSED (948 TSX files).
  - `bun run check:motion`: PASSED.
  - `bun run check:dead-css`: 本次修改文件净新增 0 个死 CSS 工具类。
- **构建验证**:
  - `bun run build:ui`: PASSED (exit code 0, 1m 54s).
  - `cargo check --workspace`: PASSED (exit code 0, 4m 28s).
- **24 项判据**: 逐条填入 `RESULTS.zh.md`，物理硬件及原生窗口效果在 `BLOCKED.md` 明确列出边界。
