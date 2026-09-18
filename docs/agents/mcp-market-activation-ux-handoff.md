# MCP 市场安装、配置与启用体验优化

## 交接状态

- 日期：2026-09-08
- 工作分支：`feat/mcp-market-activation-ux`
- 当前状态：主流程代码已落地；同日晚些时候，详细执行计划的阶段一（后端激活编排）与阶段二（市场/已安装页交互）已经实施并通过验证，工作树仍有未提交修改（见"执行进度"小节）。
- Git 约束：本次未提交、未推送、未创建 PR；后续 agent 继续在当前分支核验和收尾。
- 设计依据：`impeccable` 的 Quiet Kinetic Workspace 规范，以及项目现有扁平、克制、桌面工具化的交互风格。

## 目标与不变量

用户看到的流程应明确为：

`导入配置 → 查看/补全配置 → 测试连接 → 全局启用 → 会话或设定选择 → Agent 加载工具`

已实现的核心不变量：

1. 市场导入是配置导入，不是下载或立即可运行的安装；导入后始终 `enabled=false`，不会执行本地命令，也不会自动测试。唯一例外是用户在确认页显式选择"添加并启用"：导入仍停用，随后由服务端按持久化 ID 执行一次测试，成功后自动启用（阶段一/二已实现）。
2. 服务端是启用安全门禁：只有 `last_test_status=connected` 才允许从停用切换为启用；未测试或测试失败返回 `409 Conflict` 和明确原因。
3. 停用始终允许。停用不主动终止已经启动的 Agent；从下一次 Agent 构建或运行开始，不再注入该 MCP。
4. 修改 command、args、URL、环境变量、Header 或原始配置后，自动停用，清除测试状态、连接时间和旧工具列表，必须重新测试。
5. Conversation、ACP、Nomi 三条加载链都再次检查数据库中的 `enabled`，不能被旧会话快照或旧设定引用绕过。
6. 会话和设定的新选择只展示已启用 MCP；旧设定中的停用引用保留为可移除的“已停用、运行时不会加载”项。
7. 测试发现的 `tools/list` 结果与当前会话实际加载是两个概念，管理页只把前者标记为“发现的工具”。

## 已完成的修改

### 后端

- `crates/backend/nomifun-mcp/src/service.rs`
  - 市场 batch import 强制以停用状态保存。
  - toggle 启用前校验最近一次连接测试成功；停用不受此限制。
  - 配置发生实质变化时清除旧测试和工具发现结果并停用。
- `crates/backend/nomifun-db/src/repository/mcp_server.rs`
  - 为内部更新参数补充 `last_test_status` 和可清空的 `last_connected`；没有改变对外 MCP DTO。
- `crates/backend/nomifun-db/src/repository/sqlite_mcp_server.rs`
  - 持久化测试状态清除逻辑，并增加配置变化后的状态清除测试。
- `crates/backend/nomifun-conversation/src/service.rs`
  - 会话快照筛选已启用、非 builtin、被选择的数据库 MCP；增加停用引用不会进入快照的测试。
- `crates/backend/nomifun-ai-agent/src/factory/mod.rs`
  - 增加共享的 session MCP enabled 硬门禁；数据库读取失败时对已知快照 ID fail-closed，未知 ID 保留给 session-only/扩展贡献。
- `crates/backend/nomifun-ai-agent/src/factory/acp.rs`
  - ACP 的用户数据库 MCP 和 session snapshot 都执行 enabled 校验。
- `crates/backend/nomifun-ai-agent/src/factory/nomi.rs`
  - Nomi 的用户数据库 MCP 和 overrides snapshot 都执行 enabled 校验。
- `crates/backend/nomifun-api-types/src/mcp.rs`
  - 仅补充契约说明，没有新增字段或数据库迁移。
  - 阶段一新增 `McpTestByIdResponse` 与 `McpActivationResponse`（含 `enable_rejected_reason`）。
- `crates/backend/nomifun-mcp/src/activation.rs`（阶段一新增文件）
  - `McpActivationService`：`test_server_by_id` 按持久化配置执行测试并持久化结果；`test_and_enable` 显式激活编排。
  - 配置漂移保护：测试期间配置变化时，不持久化过期结果、保持停用并返回原因。
  - `McpConnectionTester` trait 便于集成测试注入 mock。
- `crates/backend/nomifun-mcp/src/service.rs`（阶段一追加）
  - `enable_server`：独立幂等启用入口，复用 `last_test_status=connected` 门禁；不会误停用。
  - `get_server_model`：返回结构化 transport 的域模型查询。
- `crates/backend/nomifun-mcp/src/routes.rs`（阶段一追加）
  - 新路由：`POST /api/mcp/servers/{id}/test`、`POST /api/mcp/servers/{id}/activate`；测试失败为数据（HTTP 200 内嵌结果）而非 HTTP 错误，与临时测试路由语义分离。
- `crates/backend/nomifun-app/src/router/state.rs`
  - `build_mcp_state` 装配共享的 `McpActivationService`（与连接测试服务同一 tester 实例）。
- `crates/backend/nomifun-mcp/tests/activation_integration.rs`（阶段一新增文件）
  - 7 个集成测试：成功激活启用并持久化工具、失败/需认证保持停用、配置漂移拒绝启用、按 ID 测试持久化（含失败与"测试通过但不启用"）、404。
- `crates/backend/nomifun-mcp/tests/service_integration.rs`
  - 适配新的导入/启用契约并覆盖服务集成路径。
- `crates/backend/nomifun-app/tests/mcp_crud_e2e.rs`
  - E2E 覆盖“未测试不能启用 → 成功测试后可启用 → 可停用”；测试通过直接写入测试成功状态作为 fixture，避免把 E2E fixture 误当成连接探测。
  - 阶段一追加 3 个用例：激活失败路径（真实执行不存在的 stdio 命令，保持停用且原因可读）、按 ID 测试失败持久化、激活 404。

### 前端

- `ui/src/renderer/pages/mcp/McpMarketSettings.tsx`
  - 将 CTA 统一为“导入配置”；详情不再展示通用安装命令。
  - 导入确认页展示来源、服务名、transport、stdio command/args/env key、HTTP/SSE URL/header key、缺失项和本地命令风险。
  - 使用三阶段流程提示，导入完成后进入已安装列表并显示等待配置和测试。
  - 市场来源写入 `_nomifun_market`；远程配置按不可信输入处理，不执行命令或自动测试。
  - 阶段二改为双 CTA：主按钮“添加并启用”、次按钮“仅导入”；两者都以停用状态导入，前者通过导航 state 携带一次性 `activation` 上下文（`operationId` + `serverIds`），后者只携带 `mcpFocusIds`。
- `ui/src/renderer/pages/mcp/index.tsx`
  - 以 ref 快照一次性消费市场导航 state 并用 replace 清除，防止返回/前进/重挂载重复触发自动激活；向下传递 `pendingActivation` / `pendingFocusIds` / `onPendingConsumed`。
- `ui/src/renderer/components/settings/SettingsModal/contents/ToolsModalContent.tsx`
  - 已安装列表接入每行 toggle 和独立 loading 状态。
  - 阶段二新增一次性自动激活 effect：列表加载后展开目标行并逐个调用 activate（服务端门禁生效）；仅导入路径只展开并滚动到目标行。
- `ui/src/renderer/hooks/mcp/useMcpServerCRUD.ts`
  - 持久化响应优先于旧 fallback，避免后端重置状态被前端旧状态覆盖。
  - toggle 走服务端接口并显示冲突原因；市场导入不自动 toggle/测试，手动添加编辑的既有自动测试包装保持不变。
  - 阶段二新增 `handleActivateMcpServer`：只传 `mcp_server_id` 调用 activate 接口，合并持久化状态并按 `enabled`/`enable_rejected_reason` 提示。
- `ui/src/common/adapter/ipcBridge.ts`
  - mcpService 新增 `testServerById`、`activateServer`（请求体只含 `mcp_server_id`，不含 transport）与 `McpConnectionTestResultDto` / `McpActivationResult` 类型。
- `ui/src/renderer/pages/settings/ToolsSettings/McpServerHeader.tsx`
  - 用户 MCP 卡片显示启用/停用、传输方式、来源、命令或 URL 摘要、连接状态、工具数和最近检查时间；停用操作收进更多菜单。
  - 阶段二新增文字状态 CTA（`StatusActionCta`）：待检查→检查连接、检查中→正在检查并启动、失败→重试、通过且停用→启用、已启用→重新检查；激活期间卡片和任务提示同步显示 loading。
- `ui/src/renderer/pages/settings/ToolsSettings/McpServerItem.tsx`
  - 折叠面板改挂 `McpServerDetails`，并透传 `isTogglingEnabled` / `onToggleEnabled`。
  - 阶段二在根节点加 `data-mcp-server-id`，作为聚焦滚动钩子。
- `ui/src/renderer/pages/settings/ToolsSettings/McpServerDetails.tsx`（新增文件，当前为 untracked，提交时需 `git add`）
  - 展开区域显示 transport、实际 command/URL、Agent 当前可用性和已发现工具数量；替代原 `McpServerToolsList` 在折叠面板中的位置。
- `ui/src/renderer/pages/settings/ToolsSettings/McpServerItem.tsx`
  - 折叠面板改挂 `McpServerDetails`，并透传 `isTogglingEnabled` / `onToggleEnabled`。
- `ui/src/renderer/pages/settings/ToolsSettings/McpServerToolsList.tsx`
  - 无工具时提供明确空状态和下一步提示；有工具时以扁平分隔列表展示名称和描述。
- `ui/src/renderer/pages/settings/components/JsonImportModal.tsx`
  - 配置采用“摘要 + 原始 JSON”；摘要只展示敏感字段名称，不回显密钥值。
  - 原始 JSON 可编辑，提示可能包含 token/key；实质变化时明确提示保存后会停用并要求重新测试。
- `ui/src/renderer/hooks/mcp/catalog.ts`、`ui/src/renderer/pages/guid/GuidPage.tsx`
  - 新会话 MCP 选择器只提供 enabled 项。
- `ui/src/renderer/pages/settings/PresetSettings/PresetEditDrawer.tsx`
  - 新选择只允许 enabled 项；旧设定引用的停用 MCP 保留、标注停用并允许移除。
- `ui/src/renderer/services/i18n/locales/zh-CN/settings.json`
  - `ui/src/renderer/services/i18n/locales/en-US/settings.json`
  - `ui/src/renderer/services/i18n/i18n-keys.d.ts`
  - 补齐中英文市场流程、启用状态、配置摘要、测试门禁、工具空状态和停用引用文案。
  - 阶段二新增：`mcpMarket.importOnly` / `mcpMarket.addAndEnable`、`mcpActivatedSuccess`（含发现工具数）、`mcpTestFailed`、`mcpStatusCtaTesting/Enable/Retry/Check`。
- 结构测试覆盖：
  - `ui/src/renderer/hooks/mcp/catalog.test.ts`
  - `ui/src/renderer/pages/mcp/McpMarketSettings.test.ts`
  - `ui/src/renderer/pages/guid/components/GuidActionRow.mcpCheckboxSelection.test.ts`
  - `ui/src/renderer/pages/settings/PresetSettings/configOneMcp.structure.test.ts`
  - `ui/src/renderer/pages/settings/ToolsSettings/mcpInstalledLayout.structure.test.ts`
  - `ui/src/renderer/pages/settings/ToolsSettings/mcpActivationFlow.structure.test.ts`（阶段二新增：activate 端点只收 ID、一次性消费导航 state、文字状态 CTA、聚焦滚动钩子）

## 验证证据

以下检查已经在本次工作树上通过：

- `cargo test -p nomifun-db sqlite_mcp_server`
- `cargo test -p nomifun-mcp`
  - 单元测试 246 个通过；相关 integration suites 全部通过。
  - 阶段一追加 `activation_integration` 7 个用例全部通过。
- `cargo test -p nomifun-ai-agent factory::acp::tests::load_user_mcp_servers_keeps_enabled_as_a_hard_gate --lib`
- `cargo test -p nomifun-ai-agent factory::nomi::tests::disabled_user_mcp_never_passes_the_runtime_gate --lib`
- `cargo test -p nomifun-conversation disabled_selected_mcp_is_excluded_from_conversation_snapshot --lib`
- `cargo test -p nomifun-app --test mcp_crud_e2e`：32 个用例全部通过（含 toggle 门禁与阶段一新增的 3 个激活用例）。
- `cargo fmt --check` 与 `cargo check -p nomifun-app --tests` 通过（阶段一后复验）。
- 前端定向测试：阶段一/二执行后为 6 个结构测试文件共 18 个测试通过（`bun test` 直接运行；2026-09-08 复测通过）。
- `bun run gen:i18n` 与 `bun run check:i18n` 通过（阶段二后复验）。
- 前端定向测试：上述 5 个结构测试文件共 13 个测试通过（`bun test` 直接运行；交接核验于 2026-09-08 复测通过）。
- `bun run gen:i18n`
- `bun run check:i18n`
- `bun run check:theme`
- `bun run check:icons`
- `bun run build:ui`

依赖目录此前缺少已在 `ui/package.json`/`bun.lock` 声明的包，已执行 `bun install --frozen-lockfile` 补齐；没有计划性地修改依赖声明或 lockfile。

## 已知基线问题与未完成验证

这些事项不要在交接时误报为本次改动失败：

1. `bun run typecheck` 仍受既有视频生成相关类型错误阻塞，主要是自定义按钮类型上的 `disabled` 属性错误；本次修改的 MCP 文件未出现新的 typecheck 报错。
2. `bun run check:button-layout-contract` 只命中无关的既有问题：`ui/src/renderer/pages/meeting/MeetingPage.tsx:98` 缺少 `.flowy-icon-text-btn`。
3. `cargo fmt --all -- --check`：阶段一/二执行后已对涉及 crate（nomifun-mcp / nomifun-api-types / nomifun-app）运行 `cargo fmt` 并通过 `--check`；交接 agent 仍需在最终工作树上运行全量检查。
4. 真实市场来源的联网验证尚未完成。当前测试是结构/本地服务验证，不能把 mock 或静态配置结果描述成真实市场验收。
5. 浏览器视觉验收尚未完成：已创建临时 in-app browser tab 访问 `/mcp?view=installed`，当时页面仍处于 loading，后端是否完全 ready 需要交接 agent 重新确认。此前启动的 `bun run dev:web` PTY session id 为 `60657`，交接核验时端口 5173 已无监听，该 session 已终止，需重新启动。

## 横向调研：MCP 市场、存储与验证方案

### 调研结论

用户反馈的现象属实，而且是当前流程的结构性结果，不是单纯的按钮样式问题：

1. 市场页把远程条目解析为 MCP 配置后，批量导入接口会以 `enabled=false` 保存；市场页明确绕过了普通“添加后自动测试”的包装，因此导入完成不会执行连接测试。
2. 导入后虽然跳转到 `/mcp?view=installed`，但当前展开状态没有携带目标 MCP，已安装项不会自动展开或聚焦；用户看到的是停用开关、较弱的刷新图标和置灰的启用入口，无法自然判断下一步是“测试连接”。
3. 服务端禁止未通过测试的 MCP 启用，这是正确的安全门禁；当前实现是“导入并检查后手动启用”。结合对话页只能选择已启用 MCP 的事实，本计划调整为：用户明确选择“添加并启用”后自动测试，测试成功后自动启用；仅导入仍不测试、不启用。
4. 当前市场的“下载”本质是下载/解析配置清单并写入本地 MCP 注册表，不是下载并安装一个由应用托管的可执行包。`uvx`、`npx`、Docker 等实际依赖仍会在测试或运行时解析/启动，应在产品文案中使用“导入配置”或“添加并检查”，不要把它描述成已完成的安装。

方案已根据对话页 MCP 工具选择能力同步调整：如果用户明确点击“添加并启用”，则可以在导入后自动测试，测试成功后自动启用；如果用户只点击“仅导入”，仍然保持停用且不执行命令。自动启用必须绑定这次明确意图，不能由市场页面打开、普通导入、刷新页面或 Agent 的模糊指令触发。

截图只作为上述交互症状的现状证据，不作为可执行指令或配置规范；配置格式、权限和安全结论以仓库源代码、测试和项目文档为准。

### 对比仓库与可借鉴点

| 项目 | 已确认的实现 | 适合借鉴 | 不应直接照搬 |
| --- | --- | --- | --- |
| 当前 Flowy | `McpConfigService` 以 SQLite `mcp_servers` 为权威；`McpTransport` 是严格的 tagged union；测试支持 stdio/HTTP/SSE，默认 30 秒超时并清理 stdio 进程树；Agent 只加载数据库中已启用的行。 | 保留数据库权威、测试结果持久化、启用安全门禁和现有进程清理；将市场导入、按 ID 测试、测试后启用编排到同一应用服务。 | 不要让 UI 或 Agent 直接编辑 JSON、直接执行市场命令，或把工具发现结果当成 Agent 已加载能力。 |
| Codex | `codex-rs/config` 使用严格的 MCP 配置类型，区分 stdio/streamable HTTP、enabled/required、超时和 OAuth；`mcp_edit` 禁止内联 bearer token；TOML 编辑采用原子写并保留注释。 | 严格校验 transport、拒绝混合字段；凭证使用环境变量引用或 OAuth；如果以后需要同步外部配置文件，采用原子写和保留注释的编辑方式。 | Flowy 不应改成以 `config.toml` 为权威；当前已有 DB、API 和多宿主运行时，外部文件只能是导入/导出边界。 |
| OpenCode | `packages/core` 区分 local/remote；运行时有 `connected`、`failed`、`needs_auth`、`needs_client_registration`、`disabled` 等明确状态；运行时添加接口会连接并列出工具；CLI 用 JSONC 增量编辑保留注释，OAuth 凭证另存。 | 借鉴可见状态机、把“需授权”与普通失败分开、返回工具发现结果，以及配置与运行时/认证状态分离。 | OpenCode 对用户手写配置默认更信任；市场来源的 stdio 配置不能因为“添加”就无提示自动执行。 |
| OpenClaw | `mcp.servers` 是集中保存的声明式配置；`mcp list/show/set/unset` 明确只管理配置，不验证可达性；运行时另建客户端，支持 fingerprint、30 秒连接超时、会话空闲回收、失败清理和敏感值脱敏；迁移支持预览、备份和 apply。 | 借鉴 canonical normalize、配置/运行时分离、诊断摘要、URL/header 脱敏、环境变量过滤、失败清理和迁移前备份。 | 不要把 OpenClaw 的 JSON 配置当作 Flowy 的主存储；`openclaw mcp serve` 是 OpenClaw 对外提供 MCP 的服务端能力，不是市场安装器。 |
| nomifun-desktop | `docs/guides/mcp-and-skills.zh.md` 描述了添加/导入、测试、启停、同步和对应 API；其 MCP 结构与当前 Flowy 同源，未发现可替代当前 DB 模型的另一套市场存储。 | 继续复用同源的 API、DTO 和服务边界；对外文档统一“测试连接、保存结果、启用”的语义。 | 不要把同源实现误认为已经解决了市场导入后的引导问题；它不能替代当前 UI 的自动聚焦、状态 CTA 和 Agent 编排。 |

本次检查的本地证据入口：

- Flowy 市场导入：`ui/src/renderer/pages/mcp/McpMarketSettings.tsx`、`ui/src/renderer/hooks/mcp/useMcpServerCRUD.ts`。
- Flowy 测试与启用门禁：`ui/src/renderer/hooks/mcp/useMcpConnection.ts`、`crates/backend/nomifun-mcp/src/service.rs`、`crates/backend/nomifun-mcp/src/connection_test/mod.rs`。
- Flowy Agent 加载边界：`crates/backend/nomifun-ai-agent/src/factory/mod.rs`、`crates/backend/nomifun-ai-agent/src/factory/acp.rs`、`crates/backend/nomifun-ai-agent/src/factory/nomi.rs`。
- Flowy Agent MCP 能力：`crates/backend/nomifun-gateway/src/caps_mcp.rs`、`crates/backend/nomifun-gateway/src/deps.rs`。
- Codex：`D:\workSpace\git_clone_test\codex\codex-rs\config\src\mcp_types.rs`、`mcp_edit.rs`、`core/src/config/edit.rs`、`cli/src/mcp_cmd.rs`。
- OpenCode：`D:\workSpace\git_clone_test\opencode\packages\core\src\v1\config\mcp.ts`、`packages/opencode/src/mcp/index.ts`、`packages/opencode/src/cli/cmd/mcp.ts`。
- OpenClaw：`D:\workSpace\openclaw\docs\cli\mcp.md`、`src/config/mcp-config.ts`、`src/agents/agent-bundle-mcp-runtime.ts`、`src/commands/migrate`。
- 同源桌面实现：`D:\workSpace\git_clone_test\nomifun-desktop\docs\guides\mcp-and-skills.zh.md`。

仓库状态也已记录：Codex 工作树干净；OpenCode 是完整仓库，当前为 `dev` 分支且工作树干净；nomifun-desktop 存在与本任务无关的脏文件；OpenClaw 本地 `main` 落后远端且有未跟踪目录。后续复核不能把这些本地快照状态误写成产品能力结论。

### 适合当前项目的市场与下载/使用方案

建议把市场设计成“声明式 MCP 清单 + 本地配置注册表”，分开处理三件事：

1. **发现**：远程市场只提供 `id/slug/name/version/source/license/homepage/transport/凭证需求/风险/兼容性/安装提示` 等 manifest。远程响应按不可信输入处理，缓存与已安装数据库分离。
2. **导入**：严格解析 manifest，展示来源、transport、command/args 或 URL、环境变量/Header 的 key 名称、风险和缺失项；只把配置写入本地 DB，初始始终停用。当前 `_nomifun_market` provenance 放在 `original_json` 中可继续作为过渡，运行时不得以 raw JSON 代替结构化配置。
3. **运行**：只有用户明确测试并通过后才能启用；测试按 DB 中的 `mcp_server_id` 读取权威配置，不信任客户端或 Agent 重新提交的 transport。成功只表示握手和 `tools/list` 通过，不表示已经被某个会话加载。

推荐的单条“添加并启用”路径：

`市场条目 → 配置预览 → 用户确认添加并启用 → 导入停用 → 跳转已安装/自动展开 → 自动测试 → 成功后自动启用 → 出现在对话 MCP 选择器`

其中“添加并启用”必须是显式授权后的主 CTA；“仅导入”作为次要操作保留。导入阶段仍先写入停用记录，避免测试过程中的中间状态被 Agent 使用；测试成功后由服务端在同一激活意图下通过现有启用门禁。这样用户只需等待一次明确的测试结果，成功后即可在对话页选择该 MCP；失败或缺凭证时仍保持停用并要求处理。

已安装列表应把刷新图标升级为可理解的状态 CTA，并携带路由 focus/expand 目标：

| 状态 | 用户看到的主操作 | 说明 |
| --- | --- | --- |
| 未检查 | 检查连接 | 可展开查看配置摘要 |
| 检查中 | 正在检查 | 显示进度，防止重复点击 |
| `connected` | 启用 / 已启用 | 手动测试流程显示“启用”；“添加并启用”流程成功后显示“已启用” |
| `needs_auth` / 缺凭证 | 登录或补充凭证 | 不能承诺只等待即可完成 |
| `failed` | 重试 / 编辑配置 | 显示可操作的错误原因 |

批量导入应逐项显示测试进度和部分成功结果；不能用一个全局 toast 覆盖“已导入、未测试、测试失败、需授权”这几种不同状态。

真正的包下载/安装应另立产品能力：只有在需要固定版本、校验和、签名验证、应用托管缓存或隔离运行环境时，才引入 artifact manifest 和 app-owned cache。当前市场先维持配置导入，避免把运行时包管理、供应链校验和 MCP 配置生命周期混在一起。

### 通用代码与 Agent 聊天操作

现有普通添加/编辑/批量导入路径已经有“持久化后测试”的部分包装，但市场页直接使用 CRUD，导致行为不一致。建议在现有 `McpConfigService` 和 `McpConnectionTestService` 之上补一个薄的应用编排边界（名称可按项目惯例调整），只承载以下复用动作：

- `resolve_market_entry`：获取并校验市场配置，返回预览数据和 provenance。
- `import_disabled`：写入 DB 并返回真实持久化行及 ID。
- `test_server_by_id`：从 DB 读取配置、执行测试、持久化测试状态和工具发现结果。
- `enable_after_test`：复用现有服务端门禁，只有 `connected` 才允许启用。

HTTP/UI、普通设置页和 Gateway/Agent 都调用这组应用动作；UI hook、Agent capability 不重复实现 stdio/HTTP/SSE 握手、超时、清理和状态持久化。`GatewayDeps` 目前只有 MCP 配置服务，接入按 ID 测试时需要以同一服务实例补齐测试器依赖，而不是在 gateway 中另写协议实现。

Agent 聊天已经具备 owner-only 的列出、添加、编辑、删除、启停能力，但目前没有把连接测试作为 MCP capability 暴露。建议按现有 danger tier 增加：

- `nomi_mcp_preview_market`：Read，仅返回脱敏预览。
- `nomi_mcp_import`：Sensitive，需要确认，写入后保持停用。
- `nomi_mcp_test_connection`：Sensitive 或更高等级，参数只接受 `mcp_server_id`；是否允许 Channel/Remote 面调用需按 capability matrix 收紧。
- 复用现有 toggle capability：测试成功后由用户明确确认启用。

聊天编排应为：`预览 → 说明来源/命令/风险 → 请求确认 → 导入停用 → 按 ID 测试 → 汇报状态和工具数 → 请求是否启用`。禁止 Agent 通过 shell、任意 JSON 或直接数据库写入绕过服务端校验。这样既支持后续“通过 Agent 聊天加入 MCP”，又保持 UI、HTTP 和 Agent 的配置结果一致。

### 分阶段落地建议

- **P0 交互闭环**：导入后携带 focus/expand 目标；增加“添加并启用”和“仅导入”双 CTA；已安装行显示文字状态 CTA；测试按 ID 读取 DB；明确激活意图下成功后自动启用。
- **P1 Agent 闭环**：复用应用编排服务，加入市场预览、确认导入、按 ID 测试和结果汇报能力；保留 owner scope 与 danger tier。
- **P2 凭证和批量体验**：引入 `needs_auth`/OAuth 或凭证引用、环境变量过滤、敏感值脱敏、批量逐项进度与部分成功恢复。
- **P3 真正包安装（按需）**：版本锁定、checksum/signature、隔离缓存和卸载/升级生命周期；没有明确需求时不提前引入。

### 验证边界

本节是基于本地源代码、测试和文档的横向调研，未宣称真实市场联网或浏览器验收已经完成。Codex、OpenCode、OpenClaw 和 nomifun-desktop 的借鉴结论是静态源码证据；OpenClaw 本地快照落后远端，不能据此断言其当前线上版本行为。真实市场来源、网络失败、OAuth、Windows 本地命令权限和完整 UI 视觉流程仍需单独验收。

## 详细执行计划（已批准执行）

### 计划状态与范围

- 2026-09-08 交接核验：本计划引用的现状声明已逐项对照源代码确认属实（分支与工作树、Agent capability 集合与 SKIPPED 的 `nomi_mcp_test_connection`、`GatewayDeps` 仅有 `mcp_config_service`、`batch_import` 强制停用、toggle 门禁 409、配置变更清除测试状态、市场页直用 CRUD、`_nomifun_market` provenance、owner-only 域注册）。计划经负责人批准，开始按阶段执行；执行过程同步在"执行进度"小节记录。
- 这是对后续实现的执行计划，不代表本节已经实施；生成本节时不修改业务代码、不运行实现性测试、不提交、不推送。
- 实施前先冻结当前工作树边界。当前分支已有 MCP 相关未提交修改和新增文件，必须逐项确认归属，不能用 reset、stash、清理未跟踪文件等方式整理现场。
- 目标是打通“市场明确添加并启用 → 自动测试 → 成功自动启用 → 对话可选”，而不是把所有 MCP 运行时、凭证和真正包管理一次性重构。

### 核验补记（2026-09-08 交接复核）

以下事实均已对照源代码确认，作为本计划的前提依据：

| 计划声明 | 核验结果 |
| --- | --- |
| 分支有 MCP 未提交修改和新增文件 | ✅ `feat/mcp-market-activation-ux`：28 个 MCP 相关修改文件 + 2 个 untracked（`McpServerDetails.tsx`、本文档） |
| Agent 已有 list/add/edit/delete/toggle，无测试 capability | ✅ `caps_mcp.rs:384-425`；`nomi_mcp_test_connection` 列于 SKIPPED（caps_mcp.rs:525） |
| `GatewayDeps` 只有 MCP 配置服务 | ✅ `deps.rs:92` 仅 `mcp_config_service`，无连接测试器 |
| 导入始终停用 | ✅ `service.rs` `batch_import` 强制 `enabled=false`；测试 `batch_import_always_starts_disabled`（service.rs:1248） |
| 只有 `connected` 才能启用，返回 409 | ✅ `service.rs:170-174`；`McpError::Conflict → AppError::Conflict`（error.rs:44） |
| 配置变更后清除测试状态并停用 | ✅ `service.rs:133/275` |
| 市场页直接使用 CRUD、无测试编排 | ✅ `McpMarketSettings.tsx` 仅调用 `handleBatchImportMcpServers`，把 ID 交给已安装页激活 |
| `_nomifun_market` provenance 在 `original_json` | ✅ `McpMarketSettings.tsx:35/60/168` |
| Agent MCP capability 为 owner-only | ✅ `caps_mcp::register` 经 `register_instance_owner_domain`（registry/mod.rs:181），域级统一 `AccessScope::InstanceOwner`，registry 层集中执行（mod.rs:230/265） |
| 三条加载链 enabled 门禁已存在 | ✅ `factory/mod.rs:46`（fail-closed）、`acp.rs:348` |

补充事实（计划执行时需注意）：

1. 已保存 MCP 的 `/api/mcp/test-connection` 仍保留兼容 wire shape，但带 `mcp_server_id` 时已改为按 ID 读取数据库配置；只有未保存草稿才使用请求体 transport。
2. `test-and-enable` 组合入口已新增为 `POST /api/mcp/servers/{id}/activate`，并由 Gateway 复用同一 `McpActivationService`。
3. `default_decision` 矩阵中 Sensitive 在 Channel/Remote 默认拒绝；新增测试/激活 capability 定为 Sensitive，避免 Agent 在外部渠道执行本机命令。Desktop 仍按现有确认策略运行。
4. `mcp_servers.config_revision` 与条件更新接口保护异步测试结果，配置变更期间的旧结果不会写回。
4. 计划未覆盖“编辑配置（Write 级）绕过测试再启用”的 tier 评估；启用统一走服务端 toggle 门禁，暂不扩大范围，若产品要求更严再单独评估。

### 执行进度

- [x] 阶段一：后端统一编排与按 ID 测试（2026-09-08 完成）
  - `nomifun-mcp/src/activation.rs`（新增）：`McpActivationService` + `McpConnectionTester` trait；`test_server_by_id` 按 DB 持久化配置执行测试并持久化结果；`test_and_enable` 显式激活编排，配置漂移时不持久化过期结果并拒绝启用。
  - `McpConfigService::enable_server`：独立启用入口（幂等，复用 `last_test_status=connected` 门禁），不会误停用。
  - 新路由：`POST /api/mcp/servers/{id}/test`、`POST /api/mcp/servers/{id}/activate`（测试失败为数据而非 HTTP 错误，与临时测试路由语义分离）。
  - DTO：`McpTestByIdResponse`、`McpActivationResponse`（nomifun-api-types）。
  - 测试：`activation_integration.rs` 7 个（成功启用/失败停用/需认证/配置漂移/按 ID 持久化/404）；`mcp_crud_e2e.rs` 新增 3 个（激活失败路径真实执行 stdio 命令、按 ID 测试失败持久化、404）。
- [x] 阶段二：市场页与已安装页交互（2026-09-08 完成）
  - 市场确认页双 CTA：主按钮"添加并启用"、次按钮"仅导入"（`McpMarketSettings.tsx`）；两者都以停用状态导入，差异只在是否携带带 `operationId` 的激活上下文。
  - 导航状态一次性消费：`/mcp` 页以 ref 快照 + replace 清除 history state，防止返回/刷新重复触发（`pages/mcp/index.tsx`）。
  - `ToolsModalContent.tsx`：一次性自动激活 effect（展开 + 逐个调用 activate）；仅导入路径展开并滚动到目标行（`data-mcp-server-id` 钩子）。
  - `McpServerHeader.tsx`：文字状态 CTA（待检查→检查连接 / 检查中→正在检查并启动 / 失败→重试 / 通过且停用→启用 / 已启用→重新检查）。
  - i18n：zh-CN/en-US 新增 `mcpActivatedSuccess`、`mcpTestFailed`、`mcpMarket.importOnly/addAndEnable`、`mcpStatusCta*`；`gen:i18n` 与 `check:i18n` 通过。
- [x] 阶段三：Agent 聊天复用
  - `GatewayDeps` 注入共享 `McpActivationService`。
  - 注册 `nomi_mcp_test_connection` 与 `nomi_mcp_activate_server`，参数只接收 canonical `mcp_server_id`，不接收 transport 或配置文件路径。
  - Agent 与市场/UI 共用服务端配置读取、测试、配置版本保护和结构化结果。
- [ ] 阶段四：验证与验收（自动化完成，真实桌面视觉验收仍需执行）

阶段一/二验证证据（2026-09-08）：

- `cargo test -p nomifun-mcp`：单元 246 通过；activation integration 8 通过；其余 suites 全部通过。
- `cargo test -p nomifun-gateway`：135 个单测通过，包含新增 MCP capability schema 契约。
- `cargo test -p nomifun-app --test mcp_crud_e2e`：32 通过（含 3 个新激活用例；失败路径用例因 30s 测试超时各耗时约 60s）。
- `cargo fmt --check`（本次涉及 3 个 crate）通过；`cargo check -p nomifun-app --tests` 通过。
- 前端 MCP 定向测试 35 个通过（10 个文件，含 `mcpActivationFlow.structure.test.ts`、市场双 CTA 和卡片布局断言）。
- `bun run gen:i18n` / `bun run check:i18n` 通过。
- `bun run typecheck` 仍受既有视频生成基线错误阻塞（videoCanvas/videoGeneration），本次 MCP 文件无新报错。

### 设计决策

| 用户动作 | 是否导入 | 是否测试 | 测试成功后 | 失败/缺凭证 |
| --- | --- | --- | --- | --- |
| 仅导入 | 是，初始停用 | 否 | 保持停用，用户之后手动测试和启用 | 无测试结果 |
| 添加并启用 | 是，初始停用 | 是，按已保存 ID 自动执行 | 自动启用，进入对话 MCP 选择器 | 保持停用，显示重试、编辑或补凭证 |
| 已安装页手动测试 | 已存在 | 是 | 保持现有启用状态；停用项显示“启用” | 保持停用或保留当前状态，显示失败原因 |
| Agent 只说“添加” | 是，初始停用 | 否 | 不启用 | 回报“已导入，尚未测试” |
| Agent 明确说“添加并启用” | 是，初始停用 | 是 | 测试成功后自动启用 | 请求用户处理失败或认证问题 |

核心安全规则：只有“添加并启用”这一明确意图可以触发测试后自动启用；任何被动导航、列表加载、刷新、普通导入和模糊聊天指令都不能触发。

### 阶段一：后端统一编排与按 ID 测试

1. 复核 `nomifun-mcp` 当前配置服务、连接测试服务、导入路由和 toggle 路由，先确认现有接口能否最小扩展，避免新增重复协议实现。
2. 在现有服务边界上增加薄的 MCP setup/activation 编排能力，职责限定为：导入为停用、按数据库 ID 读取配置、执行测试、保存测试结果、测试成功后调用现有启用门禁。
3. 将已保存 MCP 的测试入口改为 ID 优先。客户端和 Agent 不得通过请求体重新提供 transport 作为已保存服务的权威配置；未保存的手动配置测试可以保留现有临时测试路径。
4. 设计接口时优先采用一个明确表达用户意图的组合入口，例如 `test-and-enable`；如果复用现有测试和 toggle 路由，必须由同一应用服务编排，不能把安全判断散落在前端。
5. 测试完成后重新读取当前记录并依赖服务端门禁确认启用。若配置在测试期间发生变化，测试状态必须被清除，启用应被拒绝。
6. 批量导入先逐项执行并返回每个 ID 的结果，不以一个整体成功标记掩盖部分失败；后续再考虑批量后端接口。

后端验收条件：普通导入仍是停用；成功的明确激活流程最终是 `connected + enabled=true`；失败永远不能启用；配置变更后必须重测；测试结果和工具列表来自实际持久化结果；所有 owner scope、danger tier 和现有 API 兼容约束仍成立。

### 阶段二：市场页与已安装页交互

1. 市场确认页保留来源、transport、命令/URL、环境变量/Header key、风险和缺失项展示。
2. 将 CTA 明确拆成：主按钮“添加并启用”，次按钮“仅导入”；用户点击主按钮前必须看到 stdio 本地执行警告。
3. 导入接口返回真实持久化 ID。前端不使用名称或临时索引推断目标 MCP。
4. 导航到已安装页时携带 `focusId`、`autoTest` 或等价的路由状态；只在这次导航状态有效时自动展开、聚焦并发起一次测试。
5. 已安装行增加测试状态和激活状态的文字 CTA，明确区分“正在测试”“测试通过”“已启用”“测试失败”；防止用户把置灰开关误认为测试按钮。
6. 测试成功后更新全局 MCP catalog 和对话选择器；如果当前对话已经创建了不可变 MCP snapshot，则提示从下一轮/新会话生效，不能伪装成当前 Agent 已经加载。
7. 测试失败、命令不存在、权限拒绝、超时、HTTP/SSE 握手失败和缺少凭证都回到可操作状态，不自动启用、不丢失配置、不覆盖其他 MCP 的结果。
8. 增加中文和英文文案，确保深浅主题、窄窗口和批量结果布局不能只依赖颜色表达状态。

### 阶段三：Agent 聊天复用

1. 复用同一 MCP setup/activation 服务，不在 Gateway 或 Agent 中实现 stdio/HTTP/SSE 连接、超时、工具发现和状态持久化。
2. 增加最小 capability 集：市场脱敏预览、确认后的停用导入、按 ID 测试并启用；普通 toggle 继续复用现有 capability。
3. 对 `nomi_mcp_test_connection` 或组合的 `nomi_mcp_test_and_enable` 设为 Sensitive 或更高等级，并只接收 `mcp_server_id`。
4. Agent 收到“添加”时不自行推断“启用”；只有用户明确表达启用意图并完成确认后，才执行测试后自动启用。
5. Agent 返回来源、最终 enabled 状态、测试状态、发现工具数量和下一步；敏感 URL、Header 值、token 和环境变量值必须脱敏。
6. 继续保留 owner-only 限制，并按 capability matrix 评估 Channel/Remote 面是否应禁止本地 stdio 的激活操作。

### 阶段四：验证与验收

自动化测试按以下顺序增加或更新：

1. `nomifun-mcp` 服务测试：成功测试后自动启用、失败保持停用、缺失配置、配置变更后重测、测试期间状态清理。
2. API/E2E 测试：导入返回真实 ID；`test-and-enable` 只能按 ID 读取配置；未测试不能启用；测试成功后可被对话目录发现。
3. 前端结构/交互测试：双 CTA、路由 focus/expand、自动测试只执行一次、成功/失败分支、批量部分成功、刷新后状态恢复。
4. Agent/Gateway 测试：owner scope、确认等级、参数校验、脱敏、成功后可见、失败不启用。
5. 项目质量门：`cargo fmt --all -- --check`、相关 Rust tests、前端定向测试、`bun run typecheck`、`bun run check`、`bun run build:ui`；既有基线失败必须单独记录，不能归因给本次功能。

手工验收至少覆盖：

- 一个可用 stdio MCP：添加并启用后测试成功，自动启用并出现在对话选择器。
- 一个失败或不存在命令的 stdio MCP：导入成功但保持停用，错误可理解且可重试。
- 一个需要认证的 HTTP/SSE MCP：不会错误显示为已启用，后续可扩展到 `needs_auth`。
- 仅导入路径：不执行命令、不测试、不启用。
- 已有对话、Preset、Conversation、ACP、Nomi 的 enabled 门禁仍然生效。
- 页面刷新、重复点击、网络超时和部分批量失败不会产生重复记录或错误启用。

### 实施交付顺序与停点

建议拆成以下可审查单元，每个单元完成后先核验再继续：

1. **后端基础**：按 ID 测试和统一激活编排，配套 Rust/API 测试。
2. **市场 UI**：双 CTA、确认警告、导入后 focus/expand/auto-test，配套前端测试。
3. **状态与对话可见性**：成功自动启用后的 catalog/snapshot 刷新、失败和缺凭证状态。
4. **Agent 能力**：复用服务、确认流程、权限和脱敏测试。
5. **真实环境验收**：本地 web/desktop、真实市场来源、Windows 命令权限和必要的 OAuth。

任何阶段如果需要修改数据库迁移、认证边界、进程运行时 allowlist 或外部配置格式，应暂停并单独确认，不在本计划内顺手扩大范围。

### 完成标准

用户明确选择“添加并启用”后，只需等待一次连接测试：测试成功，MCP 自动启用并进入对话选择器；测试失败或需要认证时，MCP 保持停用且界面明确告诉用户下一步。用户选择“仅导入”时，不执行本地命令、不测试、不启用。UI、HTTP 和 Agent 三条入口最终写入同一套数据库状态，并受到同一服务端安全门禁保护。

## 交接后的建议顺序

1. `git diff --check`、`git status --short --branch`，确认所有修改仍属于本任务且无意外文件。
2. 重新运行 `cargo fmt --all -- --check`。
3. 如需刷新前端证据，运行前端定向测试；再按项目验证阶梯决定是否运行完整 `bun run check` 和相关 Rust workspace 检查。
4. 启动或复用本地 web 开发服务，实际检查中英文、明暗主题：市场详情/导入、已安装停用态、配置摘要与原始 JSON、测试失败/成功、启用/停用、旧设定引用和新会话选择。
5. 记录真实市场来源联网验证结果；网络不可用时明确记录为未验证，不用 mock 代替。
6. 最后审阅完整 diff 和测试输出。除非负责人明确要求，不提交、不推送、不创建 PR。

## 交接注意事项

- 提交时需要 `git add` 的 untracked 文件：`ui/src/renderer/pages/settings/ToolsSettings/McpServerDetails.tsx`、`ui/src/renderer/pages/settings/ToolsSettings/mcpActivationFlow.structure.test.ts`、`crates/backend/nomifun-mcp/src/activation.rs`、`crates/backend/nomifun-mcp/tests/activation_integration.rs` 以及本文档本身。

- `filter_enabled_session_mcp_servers` 会保留未知的 session snapshot ID，因为它们可能来自 session-only 或扩展贡献；只对能在用户 MCP repository 中确认的行执行 enabled 门禁。
- 已启用 MCP 的失败重测不会主动终止正在运行的 Agent；当前实现的硬约束重点是停用后的下一次构建，以及停用到启用必须有成功测试。若产品决定“失败重测立即停用”，应单独补充服务契约和测试，不要在交接时默默改变语义。
- 只读 builtin/extension MCP 不展示用户全局开关。
- 配置摘要和确认页只展示环境变量/Header 的 key 名称，禁止在文档、截图或验收记录中写入真实密钥值。
