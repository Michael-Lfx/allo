# Personal Agent 自动化 · 分批实施与验证记录

> **状态**：实施中；2026-11-25。
> **最新验证**：2026-11-26；PA-A02 完成，累计 4/31；2026-11-25 的已执行记录继续保留。
> **计划**：[开发计划与 31 项任务书](personal-agent-automation-development-plan.md)。
> **执行规则**：每项先修改、再验证、记录证据后才标记完成；Cargo 操作串行。全部目标仍为完整首版闭环，本记录不缩小原计划。
> **代码分支**：`codex/personal-agent-automation`；实现基线 `22e6a195c72be18365229fe938984aa2af9043f8`。

## 1. 当前进度

| 阶段 | 完成/总数 | 当前状态 |
|---|---|---|
| P0-A 共享宿主与独立会话 | 3/12 | PA-A01、PA-A02、PA-A04 已实现并验证；其余未完成 |
| P0-B 手动持久化运行 | 1/6 | PA-B01 已实现并验证；其余未完成 |
| P0-C 对话工具 | 0/3 | 未开始 |
| P0-D 时间触发与恢复 | 0/4 | 未开始 |
| P0-E UI、交付与保留 | 0/6 | 未开始 |
| 合计 | **4/31** | 这是任务计数，不代表工期完成比例 |

PA-A04 已修改 core、Agent 索引和产物元数据并通过下方列出的验证。PA-B01 已完成契约 crate、独立数据集 ADR、双语数据规范登记和依赖边界验证；尚未创建 SQLite 表或生产执行接线。V2 的 24 项端到端 TC 尚未整体验收；现有回归单测通过不能代替完整自动化验收。生产自动化功能仍未开启。

PA-A02 已完成独立订阅与共享面板注入，修复补快照、初始化导航、共享项目和错误修订的竞态；最终验证及失败复现见[第 6 节](#pa-a02)。本次仅完成此批，不将后续 AppHost、SQLite、账户授权或调度计入已完成。

<a id="pa-a01"></a>

## 2. PA-A01：执行宿主接缝抽取

### 2.1 已实现改动

- 新增 [execution_host.rs](../../crates/flowy-agent/src/execution_host.rs)：私有模块内区分拥有 Runtime 和借用 Tokio Handle；拥有模式仍使用原来的 2 线程、enable_all、线程名称及正常 Drop。借用模式只释放 Handle，不关闭应用 Runtime。
- 将 model_route、prepare_skill_prompt、append_skill_listing、engine_config、tool_registry 及路径/技能/Shell guards 原样移到此模块，目录 I/O、Engine 所有权和 UI 投影留在原处。对 10 个移动函数体做了静态对比，除格式外一致。
- [handle.rs](../../crates/flowy-agent/src/handle.rs) 新增 with_config_on_runtime 与 with_config_background_on_runtime；旧构造不变，后台初始化共用同一处理，原派发点改经 ExecutionHost。
- [lib.rs](../../crates/flowy-agent/src/lib.rs) 仅登记私有模块。未修改 Engine、会话 JSON、权限、取消协议、调度或生产装配。
- 新增 4 个仅位于 cfg(test) 的隔离回归测试；没有增加生产凭证、余额、mock 配置或后台 echo 兜底。原有前台默认配置逻辑随代码原样移动。

新公共构造的生命周期约束：调用者必须让外部 Runtime 活到所有 AgentHandle 和执行工作结束；它们目前只是接缝，不表示 AppHost 或独立自动化会话已经实现。

### 2.2 新增测试

| 测试 | 证明的行为 | 最终结果 |
|---|---|---|
| borrowed_runtime_runs_and_drops_agent_inside_async_context | 可在外部 current-thread Runtime 的异步上下文中运行并销毁 Agent；Runtime 随后仍可执行任务 | 通过 |
| shared_runtime_survives_dropping_one_agent | 两个隔离数据根共用 Runtime，销毁一个不终止另一个 | 通过 |
| borrowed_runtime_supports_background_storage_initialization | 借用 Runtime 下后台初始化完成后能正常发送 | 通过 |
| borrowed_background_initialization_reports_storage_failure | 注入 sessions 路径故障后仍发布初始化完成和存储错误 | 通过 |

独立评审发现新增测试等待初始化标志与事件发布之间的竞态，已改为在有界时间内累计事件，收到 Initialized 后再断言。没有为修测试改变原初始化发布顺序。

### 2.3 实际执行的验证

工作目录均为 `C:/code/flowy/allo/flowy18_wip`；Rust/Cargo 1.100.0。先载入现有 VS 2022 x64 编译环境，TEMP/TMP 指向隔离的 build-test/personal-agent-automation/tmp；未改仓库默认 profile、registry 或工具链。以下为修改后的最终结果：

| 命令 | 退出码/结果 |
|---|---|
| cargo clippy -p flowy-agent --all-targets --locked -- -D warnings | 0，警告作为错误检查通过 |
| cargo check -p flowy-agent -p flowy-studio -p flowy-app --locked | 0，Agent 及现有 Studio/App 消费者编译通过 |
| cargo test -p flowy-agent --locked --lib -- --test-threads=1 | 0；**111 passed / 0 failed / 1 ignored** |
| rustfmt --edition 2024 --check（execution_host.rs、handle.rs、lib.rs） | 0，变更 Rust 文件格式检查通过 |
| git diff --check | 0，变更空白检查通过 |

原有忽略项为 perf_tests::offline_agent_baseline（手动性能基线），未把它计为已执行。现有 artifact integration、技能、guard、工作区恢复和交付重试回归均在本次 112 项测试集合中。

本地原始日志（.scratch 为忽略目录，不随 Git 分发）：

- `.scratch/personal-agent-automation-a01-check.log`
- `.scratch/personal-agent-automation-a01-tests.log`
- `.scratch/personal-agent-automation-a01-clippy.log`
- `.scratch/personal-agent-automation-a01-consumers.log`（此前单独的消费者编译结果）
- `.scratch/personal-agent-automation-a01-verification.json`（最终 Rust 文件 SHA-256、测试汇总和文档/归档校验结果）

首轮检查的未使用 import，以及 Clippy 对私有模块内 pub(super) 的冗余可见性提示均已修正；最终检查针对修正后的文件重新执行。模块本身仍为私有，不增加公共 re-export。

### 2.4 结论与边界

PA-A01 的接缝、兼容构造、Runtime 所有权、初始化错误和消费者可编译条件已满足，可标记完成。它只为 TC-01 提供底座，不能据此声称完整 detached/调度/账户隔离或 24 项 TC 已通过。

回退：在尚未接入生产共享宿主时，可回滚本任务三个 Rust 文件的移动和构造接缝，不涉及数据迁移。后续已有私有自动化数据时，遵守总计划的安全降级规则。

## 3. 下一批与 Git 边界

**最新交接（2026-11-26）**：PA-A02 已完成并验证，下一项按依赖顺序为 PA-A03（AppHost 与唯一存储所有者）；PA-B02 仍以 PA-B01 为前置等待后续批次。当前仍为 `codex/personal-agent-automation` 上的本地未提交修改，本次没有提交、推送或重写历史。

以下保留 2026-11-25 的交接原文；其中“下一项”是当时状态：

PA-B01 已完成并验证。下一项按执行顺序开展 PA-A02（独立订阅与 Studio 注入入口）；PA-B02 以 PA-B01 为前置，后续实现 SQLite 持久层。PA-A04 已完成，但尚未实现 AppHost、授权或调度。

已 fetch origin，分支从 origin/main 建立后仅 fast-forward 到原有 22e6a195c 基线，保持现有技能/草稿功能；没有重写历史、创建新实现提交或推送。原计划的三份未提交文档已完整恢复。

后续记录追加在本文件，保留已执行结果和失败修复过程；不把计划命令复制为通过证据。

<a id="pa-a04"></a>

## 4. PA-A04：兼容执行元数据与安全创建

### 4.1 已实现改动

- 在 `flowy-agent-core` 增加 `ExecutionOwner`、`ExecutionSource`、`ExecutionMetadata`、`SessionSpec` 和 `SessionCreateDisposition`。来源与 Home/Project 展示来源分离；Automation 必须带非空 automation/run/attempt ID 和 owner，凭证不进入该元数据；owner 的 Debug 输出脱敏。
- `SessionRecord`、`DeliveryCommit`、Agent 索引记录与 artifact manifest 使用 serde 默认值兼容旧 JSON。旧记录保持 Unknown、unowned；不根据当前账户补认领。Automation 身份贯穿 Engine、工具上下文、提交证明、artifact marker、索引恢复和会话恢复。
- 新增 `Engine::create_if_absent`：重复请求只在来源、workspace 和完整 execution binding 全等时复用；冲突返回 AlreadyExists；空/非法 ID 或无 owner Automation 拒绝。保存失败移除刚插入记录并释放锁；不覆盖 active session 或既有消息。
- 新增 core/Agent 回归覆盖旧历史、重复与并发创建、绑定冲突、写失败回滚、索引重建和产物身份传播。修正测试 fixture 的公开类型引用与断言格式。

### 4.2 实际验证

工作目录为 `C:/code/flowy/allo/flowy18_wip`；以下命令针对最终 Rust 改动执行：

| 命令 | 结果 |
|---|---|
| `cargo test -p flowy-agent-core --lib --locked -- --test-threads=1` | **60 passed / 0 failed / 2 ignored** |
| `cargo test -p flowy-agent --lib --locked -- --test-threads=1` | **118 passed / 0 failed / 1 ignored** |
| `cargo test -p flowy-app --lib artifact_tests --locked -- --test-threads=1` | **4 passed / 0 failed** |
| `cargo clippy -p flowy-agent-core --lib --locked -- -D warnings` | 0；core 生产库目标通过 |
| `cargo clippy -p flowy-agent --all-targets --locked -- -D warnings` | 0；Agent 全目标通过 |
| `cargo check -p flowy-agent -p flowy-studio -p flowy-app --locked` | 0；现有消费者编译通过 |
| `cargo fmt --check --package flowy-agent-core --package flowy-agent --package flowy-app` | 0 |
| `git diff --check` | 0 |

曾尝试对 `flowy-agent-core` 与 `flowy-agent` 一起运行 strict all-targets Clippy。新增的锁释放、并发测试收集及空集合断言问题已修正；core 仍有不属于本任务差异的测试 lint（engine/session/tool 测试中的 assert-is-empty、字段重赋值、下划线绑定、分号和字面量生命周期等）。独立审查还指出 owner Debug 可能传播账户标识；现已脱敏并新增回归测试。因此最终分别验证 core library 和 Agent all-targets；不把 core test-target Clippy 记为通过。

首次 Agent 定向测试发现 `artifact_store` fixture 漏导入 `ExecutionMetadata`；首次 App artifact 测试发现 fixture 错从未直接依赖的 `flowy_agent_core` 路径取类型。两处均改为使用实际导出并在修正后完整复跑。首次格式检查的两处 import 换行已由 rustfmt 修正。测试/格式结果均来自最终源码。

### 4.3 结论与边界

PA-A04 满足 TC-01/07/10 涉及的元数据持久兼容、相同绑定复用、冲突拒绝和内存回滚条件，可标记完成。此项不提供账户授权判定、AppHost、Detached 生命周期或自动化调度；这些仍按后续任务实现。历史中缺 owner 的自动化数据不会获得可运行身份。

回退：新来源停止创建并保留已写字段；在持久化自动化数据出现后，不得回退到会忽略 owner/binding 的旧版本。

<a id="pa-b01"></a>

## 5. PA-B01：独立存储 ADR 与接口骨架

### 5.1 已实现改动

- 新增 `flowy-automation` workspace crate，定义 automation 命令/查询、RunSpec、完成证据、取消 disposition、RunExecutor 与宿主注入端口；句柄必须显式提供服务，错误原样透传，不包含默认成功或隐式 Runtime。
- 新增具名 UUIDv7 identity 类型，并在 parse 与 serde 入口拒绝非规范 UUID；账户 authority 与外部 user ID 分开建模，session/submission/request 保持 opaque 引用。任务正文、凭证引用和主体 Debug 输出脱敏。
- 新增独立 `schema_contract` inventory，登记产品表形状、逻辑引用、侧存储关联、索引、删除/重建策略与孤儿审计；DDL 契约检查拒绝物理 FK、REFERENCES、trigger、cascade、`*_row_id` 和不合规技术主键。
- 新增根 ADR-0018，登记 GPUI 独立数据集 lineage、路径、UUID 依赖、宿主所有权和 fail-closed 边界；同步中英文数据规范及 docs 索引。
- Cargo 依赖树确认生产依赖为 `serde`、`thiserror`、`uuid` 及其传递依赖，不含 Agent、GPUI、auth UI、sqlx 或父 backend runtime。

### 5.2 实际验证

工作目录为 `C:/code/flowy/allo/flowy18_wip`；以下命令针对最终 PA-B01 源码执行：

| 命令 | 结果 |
|---|---|
| `cargo fmt --check --package flowy-automation` | 0；格式通过 |
| `cargo check -p flowy-automation --locked` | 0；crate 编译通过 |
| `cargo test -p flowy-automation --locked` | 0；**11 passed / 0 failed**，doc-tests 0 项 |
| `cargo clippy -p flowy-automation --all-targets --locked -- -D warnings` | 0；全目标严格 Clippy 通过 |
| `cargo tree -p flowy-automation --locked -e normal` | 0；依赖边界符合 allowlist |
| `git diff --check` | 0；空白检查通过 |

首轮严格 Clippy 发现的可见性、文档长度和测试辅助 lint 已修正；以上结果均在最终源码上复跑。

### 5.3 结论与边界

PA-B01 的契约、身份校验、端口对象安全、依赖边界和规范登记已满足，可标记完成。`schema_contract` 仍是静态 registry 与 DDL 形状检查，不代表存在或验证过真实数据库；SQLite schema、事务、版本识别和运行期孤儿审计留在 PA-B02。此项未启用生产自动化执行。

<a id="pa-a02"></a>

## 6. PA-A02：独立订阅与 Studio 注入入口

### 6.1 已实现改动（2026-11-26 验证完成）

- `flowy-agent` 新增 `AgentSubscription`、`AgentSnapshot` 与 `AgentUpdateBatch`；每个订阅者拥有独立 broadcast receiver 和版本游标。首次订阅、重连、lag 或版本缺口返回完整状态快照，发布锁保证通知版本和发送顺序一致。快照先取 revision，随后发生的变更仍由后续通知覆盖。
- 保留兼容 `drain_updates`，兼容队列与 broadcast 容量均为 256；流式兼容提示按 session 合并。Studio 的生产泵改用自己的订阅，消费一个面板或兼容队列不会取走另一面板的通知。
- 新增 `StudioPanel::new_with_agent`；旧同步/后台构造继续包装原初始化流程，注入句柄不重置共享 workspace、origin 或 active。窗口仍共用前台 active 语义，未引入每窗口独立前台会话。
- 快照只在 Home/Chat 内恢复聊天展示，保留 Automation/Placeholder 页和未提交草稿；初始 Home 设置使用一次性 `initial_context_generation`，已发生的项目导航不会被首次 ready 或后续补快照覆盖。
- 每批更新同步共享项目，`create_task` 在通知泵运行前也查询共享 workspace，避免新 session 的 origin/workspace 错配。存储错误修订增加而事件缺失时补发提示；同批已有错误事件时不重复补发。
- 修正三个严格 lint：新增构造文档的反引号，以及既有 `automation.rs` 的私有模块可见性和不使用 self 的静态布局函数；未改自动化草稿外观和业务含义。

### 6.2 确定性回归与失败修复

原会话已有 Agent 120 passed / 1 ignored、Studio 74 passed 和消费者 check 结果，但严格 Clippy 尚有三项错误；本次先重现相同 Clippy 失败，再对发现的行为缺陷先运行红测。未将原会话结果直接作为本次完成证据。

| 测试/场景 | 修复前观察 | 最终验证 |
|---|---|---|
| `resynchronized_snapshot_preserves_project_context` | 项目 workspace 被补快照重置为 None | 通过；workspace、generation、项目 ID 与草稿保留 |
| `resynchronized_snapshot_preserves_selected_surface` | 打开过会话后，Automation 被补快照切回 Home | 通过；Automation 与 Placeholder 均保留 |
| `storage_error_revision_is_reported_without_its_event` | 提前确认 revision 后，丢失事件的相同 revision 快照不再提示错误 | 通过；缺事件时仍提示存储错误 |
| 增强双面板主题/订阅测试 | 共享 Agent 已切 Home，但面板项目 ID 仍为 shared-project | 通过；两边各自收取更新并同步项目，独立主题和草稿继续保留 |
| `create_task_uses_shared_context_before_next_poll` | 项目 workspace 的 session 被持久化为 Home origin | 通过；通知泵前创建任务仍按共享项目绑定 origin/workspace |
| `initialized_snapshot_preserves_navigation_before_first_pump` | worker ready 后、首泵前选择的项目仍被初始 Home 配置清空 | 通过；初始 generation 变化后不再覆盖导航 |

Agent 订阅回归另验证两个消费者均收到 Initialized/Sessions/Skills/Artifacts/Session（流式提示）、旧 drain 不抢通知、销毁一个订阅者后另一方仍收到更新、断开后重连取得当前快照；慢消费者溢出和显式版本缺口均补快照，快消费者持续收到增量，丢失的存储错误保留于快照修订。上述测试均不依赖真实云模型或生产数据。

### 6.3 最终执行的验证

工作目录 `C:/code/flowy/allo/flowy18_wip`，2026-11-26（Asia/Singapore）。沿用本地 `.scratch/artifact-cargo.ps1` 设置现有 MSVC/SDK 库路径及隔离 TEMP/TMP；没有更改仓库工具链、profile、registry 或依赖。Cargo 命令严格串行，以下均在最终 Rust 源码上执行：

| 命令 | 退出码/结果 |
|---|---|
| `cargo check -p flowy-agent -p flowy-studio -p flowy-app --locked` | 0；三个消费者编译通过 |
| `cargo test -p flowy-agent --locked --lib -- --test-threads=1` | 0；**121 passed / 0 failed / 1 ignored** |
| `cargo test -p flowy-studio --locked --lib -- --test-threads=1` | 0；**79 passed / 0 failed / 0 ignored** |
| `cargo clippy -p flowy-agent -p flowy-studio --all-targets --locked -- -D warnings` | 0；全目标严格 Clippy 通过 |
| `cargo fmt --check --package flowy-agent --package flowy-studio` | 0 |
| `bun scripts/check-ui-contracts.mjs --self-test` | 0；24 fixture assertions passed |
| `bun scripts/check-ui-contracts.mjs` | 0；UI contracts passed |
| `git diff --check` | 0 |

唯一忽略项仍为 `perf_tests::offline_agent_baseline`（手动性能基线），不计为已执行。前期单独的 panel 定向验证在最终补全首次初始化导航回归前为 16 passed；最终完整 Studio 集合已包含全部 17 项 panel 测试，因此以 79 passed 为准。

本地原始证据（`.scratch` 被忽略，不随 Git 分发）：

- `personal-agent-automation-a02-clippy-before.log`：三个 lint 的失败重现。
- `personal-agent-automation-a02-regression-before.log`：四项 panel 失败；`personal-agent-automation-a02-shared-task-before.log`、`personal-agent-automation-a02-initial-navigation-before.log`：另外两处导航/创建竞态红测。
- `personal-agent-automation-a02-regression-after.log`：中间定向修复验证，最终证据以下列完整测试日志为准。
- `personal-agent-automation-a02-consumers.log`、`personal-agent-automation-a02-agent-tests.log`、`personal-agent-automation-a02-studio-tests.log`、`personal-agent-automation-a02-clippy.log`、`personal-agent-automation-a02-fmt.log`。
- `personal-agent-automation-a02-verification.json`：最终源码 SHA-256、测试汇总、文档链接/任务计数/原始架构快照保留校验。

独立只读审查发现的五类行为问题已通过以上红测与最终测试闭环；修正后的再次审查没有新增阻塞发现。

### 6.4 结论、边界与回退

PA-A02 的独立订阅、通知隔离、快照/lag/版本缺口恢复、共享注入及导航回归满足本任务完成标准，登记为完成，累计 **4/31**。支撑 TC-01/24 的订阅与多面板底座，不代表 AppHost、多进程锁、Detached、账户门禁、调度或 V2 全部 24 项端到端 TC 已验收。下一项为 PA-A03。

尚未进行真实多窗口人工操作、云模型或性能基准测试。快照目前会同步读取 artifact catalog，首次订阅或 lag 恢复的大目录 I/O/锁等待需要后续性能观察；本次未测量延迟，不将它表述为性能验收通过。生产自动化仍未启用。

回退：生产共享宿主接入前可回滚本批订阅/Panel 接缝；共享后必须保留独立订阅，不能退回多个窗口争抢一个 drain 队列。未改持久格式、未做数据迁移。本次未提交或推送。

### 6.5 本批接续前的进度快照（2026-11-25 原文保留）

以下两行为第 1 节被新进度替换的旧状态，保留用于追溯：

| 阶段 | 完成/总数 | 当前状态 |
|---|---|---|
| P0-A 共享宿主与独立会话 | 2/12 | PA-A01、PA-A04 已实现并验证；其余未完成 |
| 合计 | **3/31** | 这是任务计数，不代表工期完成比例 |
