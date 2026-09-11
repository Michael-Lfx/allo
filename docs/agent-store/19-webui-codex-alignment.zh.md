# WebUI 体验对齐：Codex app 基线

> 状态：子计划（2026-09-09）；隶属 `16-sdk-webui-site-priority-plan.zh.md` 主计划，**取代其 §3 B 章**。
> 对齐目标：**Codex app（macOS / Windows 桌面端）的交互骨架**——不是照搬其编码工具特性。
> 口径：验收按「任务闭环」而非「有没有某个控件」；排期为范围值、按实测校准。

---

## 1. 对齐基线与边界

### 1.1 参考对象

Codex app 的官方定位是「agent 指挥中心」：多项目侧栏 + 并行线程、**Review/diff 面板（行内评论、按块暂存/回退、commit/PR）**、Skills 管理界面、Automations 后台任务、worktree、集成终端、审批与沙箱模式、`Cmd+K` 命令面板。

### 1.2 对齐的五个维度

| 维度 | 内容 |
| --- | --- |
| 信息架构 | 项目（workspace）→ 线程（conversation）→ turn → item 的层级与并行组织 |
| 交互骨架 | 命令面板、中断与引导、审批、计划/待办 |
| 审查面 | 变更与产物、行内评论、接受/回退 |
| 状态与反馈 | 运行状态树、用量与上下文、通知与连接状态 |
| 输入与配置 | 附件输入、设置分区、技能管理 |

### 1.3 非目标（已确认，2026-09-09）

- **worktree / Git commit / PR**：Agent Store 工作台不承担版本控制职责。
- **IDE 扩展同步**。
- **in-app browser / computer use / 语音听写 / 浮动窗口**。
- **集成终端**（可作为只读日志视图后置评估，但不在本轮）。

> 判断规则：*这是「编码工具」特性，还是「agent 协作」特性？* 前者不对齐。

---

## 2. 对齐矩阵（Codex 要素 → 我们的现状 → 缺口类型）

图例：✅ 数据与 UI 均已有 · 🟡 数据已就绪、缺 UI · 🔴 需协议/后端增量 · ⚪ 非目标

> ⚠️ 本表是 2026-09-09 的代码核实快照。2026-09-10 已回写 W1（三行）/ W5 / W8 五项，依据见 §3 各包的「进度」与 `16` §5.2「剩余任务总表」R8–R21。

| Codex 要素 | 我们的现状 | 缺口 | 归属 |
| --- | --- | --- | --- |
| 审批（审批卡、批准/拒绝） | ✅ 已解冻（2026-09-10，R8）：`capabilities.approvals` 派生自 runtime；`run/answer-decision` + 行内审批卡（三路 CAS，无 approve-all） | ✅ 已有 | W2 |
| 产物 / 变更审查 | ⚠️ **原判断已修正（D-W5-1）**：`artifact/list` / `artifact/get` 在 `05` §8 标注**延后实现**（`capabilities.artifacts` 恒 `false`），TS 协议包无对应类型，`artifact.created` **不存在**；可用者是宿主面 `/api/fs/*` | 🟡 收敛交付（残留：按 Run 归属 / 接受 / 回退） | W5 |
| 中断与引导 | ✅ 已接（2026-09-10，W3/R9）：`run/steer` 由 composer 的「引导输入」形态驱动（Enter → `run/steer`，先读 `run/get` 版本再 CAS；终态在发请求前拒绝并说明）；`run/cancel` 在 Run 面头部 | ✅ 已有 | W3 |
| 计划 / 待办 | `plan.created` / `plan.revised` 事件已定义（`01` §8）；事件只带 `change`/`intent`，**无步骤标题**——D-W6-1 已由 additive `run/plan` 结案（快照带 `title` / `status` / 成员 / 每次尝试的原因·错误·起止时间） | ✅ 已补（2026-09-11，W4/R10）：`run/plan` 快照投影 + `run-plan.ts` 纯层（11 例）+ `RunSurface`「计划与待办」块（标题/状态/成员/修订/尝试明细）+ 与 W6 事件树**双向锚点互跳**；快照缺失时明确提示且事件树不受影响 | W4 |
| 重规划 | `run/replan` 已定义 | 🟡 待评估是否开放 | W4（后置） |
| 运行状态（多 agent 并行） | ✅ 已建（2026-09-10，W6/R11）：`lib/run-tree.ts` 事件派生树 + `RunSurface`（计划修订 / 步骤 / 尝试 / 审批 / 副作用标记 + 收起调试面板），侧栏标运行中的会话；**残留**：步骤标题 / 耗时 / 失败原因 / member 归属缺 wire 字段（D-W6-1） | 🟡 主体已交付（D-W6-1） | W6 |
| 命令面板（`Cmd+K`） | ✅ 已建（2026-09-10，W1 V1 + **V2/R19**）：`CommandPalette.tsx`；`Cmd/Ctrl+K` 与 `/` 唤起同一面板，`Sidebar` 旧监听已移除；**V2 并入会话动作与模型 / 思考等级**（分组渲染，无会话时会话行禁用并说明原因） | ✅ 已有 | W1 |
| `/` 命令 | ✅ 已接（W1 V1）：6 个内置动作 + 已 `@` 专家的 `quick_prompts` / `default_init_prompt`（经 `agent/get`） | ✅ 已有 | W1 |
| `@` 提及 | ✅ 已补（W1 V1）：输入 `@` 触发 + 前缀过滤；选中写入结构化 `mentions`；删草稿 token 同步移除结构化项 | ✅ 已有 | W1 |
| Skills 管理界面 | client 技能/专家**只读**（无 create/update） | 🔴 需协议增量 | W12 |
| 多项目并行 | workspace + conversation 已有；侧栏无「项目→线程」分组 | 🟡 信息架构 | W6 |
| 用量 / 上下文 | `context.usage` 事件已有；`ContextIndicator` 显示 token 百分比 | ✅ 已补（2026-09-11，批 6）：模型条目带 models.dev 费率与目录窗口、选择器标注、发送前模型校验、≥80% 提示 + 新建对话、**按 turn 的 token 与估算金额**（`message.activity.usage` 走 additive 投影；费率与 token 都在才显示金额，缺席只显示 token；逐轮 usage 未持久化，仅实时轮次） | W9 |
| 通知与后台任务 | ✅ 已有（2026-09-10，W8 + 批 3 R13）：`ToastHost` + 断线横幅 + 一键重连；D4=A 多标签选主（桌面通知 / 声音 / 后台 Run 提醒跨标签只触发一次，锁不可用时有确定性降级）；「安装 / 刷新完成」Toast 与「后台 Run 终态」Toast（后台时另发通知与声音，点通知把标签拉回前台）。见 §3 W8「进度」 | ✅ 已有 | W8 |
| 会话工作流（重试/编辑/重新生成） | ✅ 已建（2026-09-10，W7/R12）：错误卡「重试」+ 可重试/不可重试徽标；最后一条用户消息行内编辑后作为新一轮发送（不覆写历史）；最后一条助手回复可「重新生成」；幂等键按「未回执 → 复用、已回执 → 新键」分开处理 | ✅ 已有 | W7 |
| 附件输入 | 无（WP-7 剩余项） | 🔴 需协议加法 | W10 |
| 设置分区 | 8 个分区仅 `general` 实现 | 🔴 需新建 | W11 |
| worktree / Git / IDE 同步 / 浏览器 / 语音 / 终端 | — | ⚪ 非目标 | — |

---

## 3. 对齐工作包（四层）

### 第 1 层 · 体验骨架（决定「像不像」）

**W1 · 命令面板 V1（`/` + `@`）—— 统一输入入口**
- 现状：`Cmd+K` 只聚焦侧栏搜索；`/` 无触发；`@` 仅 `+` 菜单点选（半实现）。
- **V1 范围（本包只做这些）**：① `/` 命令面板（内置动作 + 目录 `quick_prompts` / `default_init_prompt`）；② `@` 提及（专家 / 技能 / 连接器，前缀过滤，写入结构化 `mentions`）；③ 键盘导航（↑↓ / Enter / Esc）与 IME 兼容；④ `Cmd+K` 作为同一面板的唤起键。
- **非本包范围（见 W1b）**：会话动作、模型/思考等级切换。
- 验收：输入 `/` 或 `@` 或按 `Cmd+K` 都能唤起；选择后 `mentions` 与草稿文本一致（删除 token 同步移除结构化项）；不与 `+` 目录菜单抢占；IME 组合期间不误触。
- 依赖：后端结构化 `mentions` 已存在（`agent/run`，TC-INS-007），无需协议改动。
- **进度（2026-09-10）**：✅ 已落地 V1——新增 `CommandPalette.tsx`，触发符为 `/`、`@`、`Cmd/Ctrl+K`；**查询由草稿文本在触发符之后的片段派生**，焦点始终留在 textarea，因此 IME 组合期间不触发导航（`composingRef` 守卫）也不会误提交；`↑↓ / Enter / Esc` 由 Composer 持有的光标驱动（Esc 会连带清掉半截触发符）；`@` 选中写入结构化 `mentions`（agent / skill / connector），并用 `@name` → mention 的 token 映射实现「删除 token 同步移除结构化项」；`/` 命令 = 6 个内置动作（新建对话 / 选择文件夹新建 / 应用商店 / 产物文件 / 设置 / 分享）**加上**草稿中已 `@` 到的专家的 `quick_prompts` 与 `default_init_prompt`（经 `agent/get`）。`Cmd/Ctrl+K` 从「聚焦侧栏搜索」**改为**唤起同一面板（`Sidebar` 的旧监听已移除）——这正是本条 V1 的原目标。

**W1b · 命令面板 V2（会话与模型动作）** —— ✅ 已完成（2026-09-10，批 3 / R19）
- 范围：把会话动作（新建 / 重命名 / 删除 / 归档）与模型 / 思考等级切换并入同一面板。
- 进度：会话动作与模型 / 思考等级已并入（数据层 `web/src/lib/palette-model.ts`，组件按 `groupKey` 分组渲染、禁用行给出原因）；**归档未做**——协议里没有归档/回收站（`11` §2.2 属方向四未排期），放一个点了无反应的入口比不放更糟，故按现状收敛（登记于 `16` R19 行）。
- 验证：`palette-model.test.ts` 11 例 + `CommandPalette.render.test.tsx` 3 例；web 全量 238 passed / 1 skipped。

**W2 · 审批卡（Approvals）**
- 现状：协议已定义 `approval/request`（通知）与 `approval/respond`（方法）；`capabilities.approvals` 硬编码 `false`，webui 无界面。
- 范围：① 解冻 `capabilities.approvals`（后端开关，需确认与现有安全模型一致）；② 行内审批卡（工具 / 参数摘要 / 风险标签 / 过期时间）；③ 批准 / 拒绝 / 「本会话记住」；④ 幂等与过期语义按协议（`approval_expired` / `approval_already_resolved`）。
- 验收：需要审批的任务 → 审批卡出现 → 批准 → 运行继续；拒绝 → 运行按策略终止；重复响应返回同一结果。
- 依赖：后端解冻 + 安全评审（属高优先级阻塞项）。

**W3 · 中断与引导（`run/steer`）**
- 现状：协议与 SDK 已实现并 live 验证，webui 零使用。
- 范围：运行中 composer 切换为「引导输入」形态（不打断当前 turn）；`steer` 回执可见；终态提交被拒并明确提示；与 `cancel` 入口区分。
- 验收：Run 进行中提交引导文本 → 服务端 `run/steer` 收到且后续事件体现。
- **进度（2026-09-10，批 3 R9）**：✅ 已落地。判定层 `web/src/lib/run-steer.ts`（`available` / `busy` / `terminal` / `no-run` + `isStaleRunWrite`），store 的 `steerRun` **先读 `run/get` 版本再 CAS 提交**、冲突回读后提示重试，终态在**发请求前**拒绝；Composer 在有在跑 Run 时切「引导输入」（placeholder / 发送按钮文案 / Enter / 忙碌禁用 / 错误行内），成功走既有 Toast；`cancelRun` 与 steer 分工明确（取消在 Run 面头部）。验证：`run-steer.test.ts` + `appStore.steer.test.ts` 共 12 例，web 全量 **196 passed / 1 skipped**。

**W4 · 计划与待办树**
- 现状：`plan.created` / `plan.revised` 事件已定义，无渲染。
- 范围：计划版本（plan revision）与待办项的状态树；`run/replan` 产生的历史计划可查看（不删除历史）；与 W6 的 step 树互相跳转。
- 验收：多步任务能看清「计划 → 步骤 → 完成/失败」，重规划后新旧版本都可追溯。
- **进度（2026-09-11，批 3 R10）**：✅ 已落地，**由 additive `run/plan` 解锁（D-W6-1 结案）**。① **协议**：`run/plan`（WS 臂 + HTTP `GET /api/app-server/run/{run_id}/plan` 共用 `get_run_plan_for_user`）＝ owner 作用域与 `run/get` 同一 `engine.get` 的**权威快照**，聚合引擎 `detail`：step 的 `title` / `kind` / `status` / `role`+`model`（成员归属）/ `introduced_in_revision` / `superseded_in_revision`，attempt 的 `trigger_reason` / `question` / `error` / `output_summary` / `output_files` / `tokens` / `started_at` / `finished_at`，加 `dependencies`。**有意不投影** `participant_id` / `source_agent_id`（沿用「无公开映射的内部 id 不上 wire」），`output_files` 经既有相对路径过滤。② **投影层**：`web/src/lib/run-plan.ts`（纯模块，11 例）—— 展示序号 `ordinal` 从 1 起（与引擎从 0 起的 `attempt_no` **解耦**，两者都保留）、耗时只在两端时间戳齐全时给出（进行中不给 0）、空白错误不当错误、`superseded` 与 `skipped` 归最弱档、`planProgress` 统计完成度。③ **UI**：`RunSurface` 新增「计划与待办（done/total）」块——标题、状态徽标、`role · model`、修订徽标、每次尝试的原因/耗时/token/失败文本/产物；**与 W6 事件树双向锚点互跳**（`plan-step-*` ↔ `run-step-*`，滚动 + 短暂描边高亮）；快照缺失时只说明「快照暂不可用」，事件树照常可用（**不编内容**）。④ **store**：`runPlan` / `runPlanError` + `loadRunPlan()`（与事件流并行取、切 Run 丢弃过期结果、失败保留旧快照）。验证：`cargo test -p nomifun-app --test agent_execution_decision_e2e` → **8 passed**（新增 plan 快照用例）；`run-plan.test.ts` 11 例；`RunDetail.render.test.tsx` 3 例（含待办块与双向锚点）。**历史计划可追溯**：修订号与 `superseded_in_revision` 已在快照上，事件侧的计划修订列表（R11）仍是历史序列的权威。文档：`05` §5.2、`16` R10 行、site §7.3 路由计数 45/58。

### 第 2 层 · 审查面（差异化最大的部分）

**W5 · 产物与变更面板**
- 现状（⚠️ 2026-09-10 代码核实，修正原文）：`05` §8 定义了 `artifact/list` / `artifact/get`，但**明确标注延后实现**（`05`:152 与 `TC-AS-008`：`capabilities.artifacts` 恒 `false`，且规定「Artifact 不允许任意路径读取」）；TS 协议包与 `packages/client` **无**对应类型与子客户端，`artifact.created` 事件**不存在**。真实存在的是 `RunView.output_files`（`protocol.ts:204-213`）与 client 的 `TurnResult.output_files`（`turn-result.ts:97-112`），但 webui 不跟踪 run receipt、`ConversationEvent` 也不携带产物字段。
- **范围收敛（2026-09-10 决策）**：本轮**不引入 Artifact 协议**（`16` 第 3 批边界「不碰协议」）。改用**宿主面文件服务**（`crates/backend/nomifun-file`：`/api/fs/list`、`/api/fs/read`、`/api/fs/metadata` 服务端均已有，webui 此前只接了 `browse`）按**会话 workspace** 交付产物列表 + 预览 + 下载；「按 Run 归属」与「接受 / 回退」登记为待 Artifact Phase（见 `16` 已知偏差 D-W5-1）。
- **进度（2026-09-10）**：✅ 已落地「会话 workspace 产物面板」——`ArtifactPanel.tsx` + `appStore` 的 artifact 切片（`refreshArtifacts` / `openArtifactPreview` / `quoteArtifactIntoDraft`）+ `web/src/lib/client.ts` 的 `listWorkspaceFiles` / `readFileContent` / `getFileMetadata`（宿主面 `/api/fs/*`）+ 协议类型 `WorkspaceFlatFile` / `FileMetadata` + i18n `artifact` 分区 + `.artifact-*` 样式 + Topbar 入口。**未做**：按 Run 归属、接受 / 回退（待 Artifact Phase）；列表项不含 size / type / mtime——`/api/fs/list` 只返回 `name`/`full_path`/`relative_path`，要展示需逐文件调 `/api/fs/metadata`（当前只对预览项调用）。**补记（2026-09-11，批 3 R20）**：后半句已不成立——store 新增 `artifactsMeta` + `loadArtifactMetadata()`（并发 4、失败记 `null` 不重试、切会话丢弃过期结果），列表行与预览头已渲染真实 `size · MIME · mtime`，查不到就整段不渲染；预览头无元数据时才退回按文本长度估算。验证 `appStore.artifact-meta.test.ts` **5 passed**。「按 Run 归属」与「接受 / 回退」仍待 Artifact Phase（D-W5-1）。
- 范围：按会话与按 Run 两个视角的产物列表（名称 / 类型 / 大小 / 生成 step 归属 / 时间）；预览与下载；**行内评论**（对产物/变更的评论，供后续 turn 引用）；接受 / 回退（按文件或按块，具体粒度按 `artifact/get` 能力定）。
- 验收：跑一次产出文件的 Run → 产物面板可见、可预览、可下载；评论能作为下一轮上下文引用；回退不影响历史 turn。
- 备注：**不做 Git 集成**——接受/回退作用于产物本身，不产生 commit。

**W6 · Run 状态树（Step / Attempt / Sub-agent）**
- 现状：`RunDetail.tsx` / `RunPanel.tsx` 是调试视图（raw JSON + `seq/type/payload` 表格，英文硬编码）。
- 数据基础：`01` §8 的 `resource` 维度（`run / plan_revision / step / attempt / member / approval / artifact / connector`）与事件类型齐备。
- 范围：按 step / attempt 树渲染（状态、耗时、重试次数、失败原因）；sub-agent（member）及其产出归属可见；侧栏按「项目 → 线程」分组并标记运行状态；原始事件降级为可展开的调试面板；文案全部走 i18n。
- 验收：一次多 step 的 Run 能看清每个 step 状态与重试；失败 step 可定位到 attempt 与错误；两个并行线程的状态互不干扰。
- **进度（2026-09-10，批 3 R11）**：✅ 主体已落地，**验收口径有一项受阻**（见 `16` 已知偏差 D-W6-1）。`web/src/lib/run-tree.ts` 事件派生树（运行头 → 计划修订 → 步骤 → 尝试，按 `sequence` 去重排序、并行步骤互不串线）+ `RunDetail.tsx` 重写为 `RunSurface`（状态徽标 / 计划修订 / 可展开树 / 审批行 / 取消），原始事件降级为底部收起面板，文案全走 i18n（`run.*`）；侧栏对起 Run 的会话标 `.run-dot`；删掉无人引用的 `RunPanel.tsx`。**受阻项**：步骤标题 / 耗时 / 失败原因 / member 归属在 wire 上不存在（`task.updated` / `attempt.updated` 只有标记字段、`RunEvent` 无时间戳），要满足「失败 step 可定位到 attempt 与错误」需 additive `run/plan`，已登记待决策。验证：`run-tree.test.ts` + `RunDetail.render.test.tsx`（真实组件 SSR 断言）共 15 例，web 全量 196 passed / 1 skipped。**补记（2026-09-11，批 3 R10）**：D-W6-1 已按该行建议实施 additive `run/plan` 并结案——步骤标题、失败原因、起止时间、成员归属（`role` + `model`）全部可从快照读到，`RunSurface` 新增「计划与待办」块并带双向锚点；事件树仍是「发生过什么」的权威（副作用/引导标记），两者互补。`bun run test` → **285 passed / 1 skipped（42 文件）**（含 `typecheck` 0 错、`build` ✓、`check-docs-sync` 0 drift）；`cargo test -p nomifun-app --test agent_execution_decision_e2e` → **8 passed**，`cargo test -p nomifun-app-server --lib` → **67 passed**。**注意**：改站点 §7.3 的方法计数时有**两道**防漂移守卫（`http-transport.test.ts` 的路由条数 + `docs-drift.test.ts` 引用同一数字），必须同改。

### 第 3 层 · 会话与反馈

**W7 · 消息重试 / 编辑 / 重新生成**
- 现状：仅「复制错误」按钮；`isRetryableError` 全仓 1 处使用（`CatalogView.tsx:344`）。
- 范围：失败 turn 重试（复用幂等键，避免重复副作用）；编辑后重发；重新生成（新 turn，保留原 turn）；统一「可重试」错误呈现。
- 验收：失败 turn 可一键重试且不产生重复执行；重新生成不覆盖历史；`retryable` 与不可重试错误在 UI 上区分。
- 关联：消息错误事件已带 `result_error_retryable`；`11` §2.1。
- **进度（2026-09-10，批 3 R12）**：✅ 已落地。**先修数据断链**：`message.error` 的 wire payload 本就带 `code`/`retryable`（`lib.rs:4241`），`packages/protocol` 的解码器此前只取 `message`——补上（`retryable` 为**三态**，非布尔一律 `null`，不猜测）。**幂等策略分开**（`web/src/lib/turn-actions.ts`）：`resend`（没拿到回执）复用原键；`retry`/`regenerate`/`edit`（已回执）用新键 `kind-messageId-contentDigest`。**拒绝口径**：空正文 / 找不到用户轮次 / 不可重试 / 原键已不在四种情形**不发请求**并给出原因（`missing-key` 提示改用「重新生成」）。**编排复用**：store 抽出 `submitTurn`，`send` 与四个动作共用；UI 为错误卡「重试」+ 徽标、最后用户消息行内编辑、最后助手回复「重新生成」。验证：`turn-actions.test.ts` 17 例 + `appStore.turn-actions.test.ts` 8 例 + 解码器 1 例；`bun run test` 全绿（见 `16` R12 行的原始输出）。

**W8 · 通知与连接状态层**
- 现状：无 toast/通知组件；连接状态仅 `composer-model-dot ${phase}` 一个小圆点。
- 范围：① 全局 Toast（导入/安装/刷新完成、可重试错误、后台 Run 终态）；② 断线横幅 + 手动重连（与 SDK A2 配套）；③ 多标签协调（同一会话的订阅归属）。
- 验收：断网出现横幅并可一键重连；重连后待处理请求可继续；后台 Run 完成有可点击通知；两个标签页不再互抢订阅。
- 依赖：A2（传输层重连能力）。
- **进度（2026-09-10，W8）**：① Toast 层（`ToastHost` + `pushToast`）与 ② 断线横幅 + 一键重连**已落地**——`transport.onLifecycle("closed")` 拉横幅，重连复用同一 client（真实重拨 + 重新握手），随后按 T8 调 `rearm()` 重订阅并用 `refreshConversation()` 补齐断线窗口，成功/失败走 Toast。当时 **③ 多标签协调未做**（需先拍板归属策略，属设计决策），① 中「导入 / 安装 / 刷新完成」与「后台 Run 终态」的通知也未接（catalog 只有常驻内联反馈 `market-alert` / `installResult`；run 侧连 receipt 都还没跟）。**这两项余项已于批 3 R13 补齐，见下条。**
- **进度（2026-09-10，批 3 R13，D4=A）**：**③ 多标签协调与 ① 的余项已补齐**。归属策略 = 每个标签各自订阅各自 WS；只有**逃出标签页的副作用**（桌面通知 / 声音 / 后台 Run 提醒）进选主层，`navigator.locks` 不可用时的降级阶梯（Claim 级 → 无共享存储时每标签一次，宁可重复绝不丢失；performer 拒绝不污染 key）由代码注释与单测共同钉住；跨标签**会话内** UI（toast / 断线横幅）**有意不协调**。通知侧沿用既有 Toast 机制（`Toast` 加 `params`，`ToastHost` 透传）：`catalog.marketRefreshDone` / `marketRefreshUnchanged` / `storeInstallDone` / `storeInstallReused`；「后台 Run 终态」由 `run/events` 投影（`toast.runCompleted` / `runCompletedWithFailures` / `runFailed` / `runCancelled`），标签聚焦时只出 Toast、后台时才经选主层发通知 + 声音一次（点通知把投递标签拉回前台）。选主与通知逻辑全部可注入（假 `navigator.locks` / `BroadcastChannel` / `Notification` / `AudioContext` / 假 gate / 假 client），**不依赖手工多开标签**。实现细节与原始验证输出见 `16` §5.2 R13 行。
- **收口补正（2026-09-11，W8 余项 R13）**：重连分支（② 的「一键重连」）原只按 T8 rearm 了**会话**订阅——被跟随 Run 的 `run/subscribe` 在服务端同样已被丢弃且不会自己回来，于是**断线窗口内进入终态的 Run 没有任何提示**（后台提醒最该出现的场景）。现 `connect()` 的 `lost` 分支一并 `await runSubscription?.rearm()`：复用既有 rearm 与既有通知出口，无第二条通道；回放事件按「run + 终态」幂等。`appStore.terminal-notice.test.ts` 新增 2 例（断线期间进终态→重连后仍提示一次；重连回放同一终态→不重复）。

**W9 · 模型能力与用量**
- 现状：`models/list` 已联动（`0aabe9499` / `0f2b26d69`）；`ContextIndicator` 仅显示 token 百分比，无费用、无按 turn 汇总；模型健康/能力限制未展示。
- 范围：模型可用性 / 能力限制标注；发送前兼容性校验；按 turn 的 token 与费用汇总；接近上限时的压缩 / 新会话建议。
- 验收：不可用模型在选择器中可见且有标注；不兼容组合发送前被拦截；用量按 turn 可查。
- 关联：`11` §5.1 / §5.2 / §5.3。
- **进度（2026-09-11，批 6 R14，D12=A）**：✅ ①–⑤ 全部落地（①–④ 批 3、⑤ 批 6；批 3 的「受阻」结论已撤销）。① **取价路径**（此前缺）：`conversation/model-options` 条目新增 models.dev 目录事实 `cost_input` / `cost_output` / `catalog_context_window` / `supports_vision`（additive，缺席即不上 wire；只用已缓存目录，不触网；未映射 provider 与未知模型整组缺席）。② **能力/限制标注**：`lib/model-facts.ts` 定口径（费率文本、窗口目录优先配置兜底、**只有目录明确支持才给「视觉」标签**），`ModelPicker` 渲染「费率 · 上下文 · 视觉」，未知整段不显示。③ **发送前兼容性校验**：`send()` 在发请求前拦截不在已知目录里的显式模型（目录未加载时放行），store 4 例钉住。④ **接近上限建议**：`lib/context-advice.ts` 按服务端测量值分级（80% 阈值），Composer 提示 + 「新建对话」按钮（压缩不是协议能力，故不摆假按钮）。**⑤ 按 turn 的费用 / token（批 6 落地，撤销上文「受阻」）**：源码级侦察查明运行时 `TurnCompleted` 一直带逐轮 token、中继也一直转发，**是 App Server 投影把它降级成活动标记时丢掉的**（`nomifun-app-server/src/lib.rs:4397`）——补法是在该投影 additive 带上 `usage{input_tokens,output_tokens,total_tokens}`，客户端 `parseTurnUsage` 解码、reducer 记 `stream.turnUsage`，Composer 用 `turnCostUsd`/`costText` 出「本轮 ↑X ↓Y · $Z」；**金额只在费率与 token 都在时显示**（目录无价 / 单向价 / token 未知 / 模型键未知 → 只显示 token）。边界：逐轮 usage 未持久化（重载后不显示，新一轮开始即清空）。验证（批 6 复跑）：`cargo test -p nomifun-app-server --lib` **79 passed**（新增 2 例逐轮用量投影）；web 全量 **320 passed / 1 skipped（43 文件）**，其中 `model-facts.test.ts` 24 例（含费率/token 口径 10 例）、`conversation-events.test.ts` 16 例（含本轮用量 6 例）、protocol 包解码 13 例（含逐轮 usage 3 例）；`bun run typecheck` / `bun run build` 均 exit 0。

### 第 4 层 · 输入与配置

**W10 · 附件 / 图片输入**
- 现状：全线缺口（WP-7 剩余项）。
- 范围：消息 content 的图片载体（**协议加法**，无需版本兼容设计）、后端 run/turn 处理、composer 拖拽/粘贴 UI、发送前与模型能力校验。
- 验收：拖拽与粘贴图片 → 发送 → 模型实际收到；不支持的模型在发送前给出明确提示而非静默失败。
- 依赖：需先定 content 模型；与协议**词汇对齐**的命名改动有交集，但**不依赖它**——载体选型单独拍板后即可做加法（见 `22-webui-productionization.zh.md` §5）。

**W11 · 设置 Dialog 补全**
- 现状（R16 之前）：8 个分区仅 `general` 实现，其余 7 个是「即将推出」占位。
- **进度（2026-09-11，批 4 / `16` R16）：nav 收敛为「通用 + 供应商」两分区**。provider 分区已接真实读写——`config/get` / `config/set`（宿主管理面：WS-only、无 HTTP 绑定、**不进 SDK 包**）读写 `~/.agent-store/config.toml` 的 `default_model`（白名单字段、`deny_unknown_fields` 拒越界键、最小改动保留注释、写后重读回显）；前端 `web/src/store/settingsConfig.ts` 是**单一来源**，失败行内报错 + 重试，成功显示服务端回读值；`general` 里从不回写任何地方的 `providerId` / `model` 输入框**已删除**。其余六分区（agent / account / plugin / advanced / lab / archived）按 §6 不渲染，逐条理由与解锁条件见 `16` 文末「卡点决策」表（要点：`agent` 的唯一候选开关 `[memory] distill_enabled` 宿主只解析不消费 → 写它会成假开关）。
- 范围（按优先级）：① provider 管理（增删改、默认模型、健康状态）；② 账户；③ 插件 / 市场；④ 高级（数据目录、日志、协议版本）；⑤ 实验室 / 归档（可长期占位）。
- 验收：每个落地分区都有真实数据源与回写路径（**不留假开关**）；与宿主管理面边界一致（`~/.agent-store/config.toml` 为模型 provider 唯一来源）。
- 依赖：provider 分区需先定「写 `config.toml` 还是写 DB」，见主计划 Q6（已定 ①，已落地）。

**W12 · 技能 / 专家管理界面**
- 现状：client 的 `skills.*` / `agents.*` **只读**（仅 list/get），无创建与编辑。
- 范围：技能与专家的创建 / 编辑 / 删除 / 复制；与市场安装的产物区分（本地创建 vs 市场安装快照）；导入来源（CodeBuddy 目录）与手工创建的边界。
- 验收：界面创建的技能可被会话 `@` 引用并执行；删除不影响已安装的市场快照。
- 依赖：**需协议增量**（`skill/create|update|delete` 等）；与 D2 Schema 一起做更自然。

---

## 4. 非对齐项（市场管理与工程项）

**W13 · 市场管理补全**（原 `16` B7）
- 现状：4 种源添加、列表、详情、刷新、移除均已接；**client 已提供而 UI 未接**：`setMarketplaceAutoUpdate`、`importMarketplaceEntry`；未展示 `version` / `revision` / 上次刷新 / `enabled`。
- 范围：auto-update 开关；条目浏览 + 条目级导入（与 `store install-entry` 语义区分）；注册表字段展示；级联移除二次确认（列出将卸载的快照）。
- 验收：auto-update 切换后 `market/list` 回读一致；条目级导入产生带溯源快照；级联移除前明确列出影响面。
- 关联：`18-marketplace-spec.zh.md` §7 的 `auto_update` 默认值偏差（主计划 Q7）。
- **进度（2026-09-10，T14）**：✅ 四项已落地——① **auto-update 开关**（详情头部，`setMarketplaceAutoUpdate` 后回读 `market/list` 并同步详情，成功/失败走 Toast）；② **条目级导入**（每条目 `market/entry-import`，与 `store/install-entry` 明确区分：只导入带溯源的快照、不动安装态，回读 `import/list` 与市场详情）；③ **注册表字段**：展示 `marketplace_id` / `enabled` / `entry_count` / `added_at`；④ **级联移除确认弹窗**（`DialogShell`）列出将从该市场卸载的已安装条目，替代原先的 `window.confirm`，确认后调 `market/remove` 并回读列表与导入记录。**未做（待协议增量）**：~~`revision` / 上次刷新时间~~——**已补（2026-09-10，用户放开协议）**：`AppServerMarketplaceSummary` 加 `resolved_revision` / `last_checked_at`（additive），详情面板增加「源修订」「上次检查」两行，D-W13-1 的 ① 已落地；「移除前」的受影响快照清单 —— ✅ **已改为服务端前置投影（2026-09-10，批 1 / R21）**：`market/get` 的每条目新增可选 `snapshot`（含 `installed_count`），移除弹窗在打开时先刷新 `market/get` 再列出 `installed_count > 0` 的条目，条目卡的安装态也读同一字段（不再从聚合的 `store/list` 派生）。

**W14 · 消费 SDK 新能力**（原 `16` B3，工程项）
- 范围：A3 完成后 webui 事件层瘦身，统一走包内解码器。
- 验收：行为不变（回归测试通过），本地 reducer 代码量下降。
- 依赖：A3。

---

## 5. 验收口径（任务闭环）

不用「有没有某个控件」验收，用下列闭环：

| # | 闭环 | 覆盖 |
| --- | --- | --- |
| AC-1 | 新建线程 → `@` 引用一个技能 → `/` 插入快捷提示 → 发送 → 收到流式回复 | W1 |
| AC-2 | 一个需要审批的任务 → 审批卡出现 → 批准 → 运行继续 → 完成 | W2 |
| AC-3 | 运行中提交引导 → 服务端接受 → 后续输出体现引导 | W3 |
| AC-4 | 多步任务 → 计划树可见 → 重规划后新旧版本可追溯 | W4 |
| AC-5 | 产出文件的 Run → 产物面板可见 → 预览/下载 → 行内评论 → 回退 | W5 |
| AC-6 | 两个并行线程 → 各自 step/attempt 树清晰 → 失败可定位 | W6 |
| AC-7 | 失败 turn → 一键重试 → 无重复副作用；重新生成不覆盖历史 | W7 |
| AC-8 | 断网 → 横幅出现 → 一键重连 → 待处理请求继续；后台 Run 完成有通知 | W8 |
| AC-9 | 选择不可用模型 → 发送前被拦截并说明原因；用量按 turn 可查 | W9 |
| AC-10 | 拖拽图片 → 发送 → 模型收到；不支持的模型发送前提示 | W10 |
| AC-11 | 设置中修改 provider → 生效且与 `config.toml` 口径一致 | W11 |
| AC-12 | 界面创建技能 → `@` 可引用并执行 | W12 |

**验收记录（T15，2026-09-10）**

- ✅ **协议级 live 验收全绿（11/11）**：脚本 `web/scripts/sdk-live-w13-acceptance.ts`，对着真实运行时（`target/release/agent-store.exe --port 8787`，NoAuth local）跑，走的是 **Web 宿主客户端**（`web/src/lib/client.ts`，即面板与目录页实际调用的同一层），市场用**本地目录 fixture**（不依赖公网镜像）：
  - AC-5：`fs/list`（两个产物）/ `fs/read`（markdown 正文）/ `fs/metadata`（size 30、`text/markdown`）；
  - W13：`market/add`（第三方默认 `auto_update=false`）/ `market/auto-update` **回读一致** / `market/get` 条目 / `market/entry-import` 产出**带溯源快照**（`snapshot_id` + `content_digest`）/ `import/list` 回读 / `market/remove` 返回**级联影响面**（`snapshots`）/ 移除后 `market/list` 回读。
- ✅ **UI 点击与键盘级：人工实测通过（2026-09-10，用户执行）**——用户审查后确认无问题。期间修掉 3 个阻塞项（`16` 已知偏差）：**D-W13-2**（「市场源 / 导入记录」页签在 UI 上不可达）、**D-STREAM-1**（工具事件与首段思考共用轮 id，直播互相覆盖）、**D-STREAM-2**（回答完成后仍有 6~15s 收尾尾巴，已由 `[memory].distill_enabled` 配置关闭）。下列清单与期望保留作验收记录。环境：dev server `http://localhost:5174`（`defaultWsUrl()` 对 5174 自动回落 `ws://127.0.0.1:8787`）+ 运行时 `127.0.0.1:8787` + 产物目录 `%TEMP%\t15-artifacts`（`report.md` / `notes.txt`）。清单与期望：
  1. **AC-1**：输入框打 `/` → 出现「命令」面板（6 条内置动作 + 已 `@` 专家的快捷提示），`↑↓` 移动、`Enter` 执行、`Esc` 关闭并清掉半截触发符；打 `@` → 切「提及」并可按前缀过滤，选中后草稿出现 `@名称` 且发送走结构化 mentions；**删掉草稿里的 `@名称` 后该提及应同步移除**；`Cmd/Ctrl+K` 唤起同一面板；**中文输入法组合期间 Enter/↑↓ 不应导航或发送**；`+` 菜单不受影响。
  2. **AC-5**：新建对话并把工作区设为 `%TEMP%\t15-artifacts` → 顶栏「产物文件」→ 右侧抽屉列出两个文件；点 `report.md` 预览为 Markdown、点「下载」能存盘；`notes.txt` 显示为纯文本；评论框「插入到下一条消息」→ 草稿出现 `> [report.md] …` 并弹 toast；Esc / 点遮罩关闭。
  3. **W13**：应用商店 →「市场源」→ 点市场进详情：应显示 标识 / 启用状态 / 条目数 / 注册时间 / 源修订 / 上次检查（新注册的为 `—`，刷新一次后才有值）；点「自动更新：开/关」→ 文案翻转 + toast，刷新页面后一致（回读一致）；官方三个市场应为**开**（Q7 ①），第三方为**关**；条目上「仅导入」→ toast 且「导入记录」出现该快照、**安装状态不变**；「移除市场」→ 弹窗**列出将被卸载的已安装条目**（无则提示「没有已安装条目」）。
  > 预热提醒：全新 data-dir 下内置市场需后台注册（实测可达 2 分钟以上），市场源页一开始可能为空或不全，点刷新即可。
- 📌 顺带发现：仓库自带的 `bun scripts/smoke.ts --real` 在冷启动运行时上**被它自己的 15s 超时打爆**（`store/list`）——D-SDK-1 的第二个现场复现，说明该缺陷也卡我们自己的验收脚本（补记见 `16` 已知偏差 D-SDK-1）。
- ✅ **D-STREAM-2 收尾尾巴：服务端已修（R30，2026-09-11）**——`Finish` 不再等待记忆蒸馏 child，「回答完成」＝「轮次结束」。**R30 与 R31 的边界**（两者不是重复实现）：R30 是**服务端时序**修复（蒸馏改同轮次取消域内的后台 spawn，取消语义与协议均不变），R31 是**客户端文案分级**（`turn_completed` 已到但 `is_processing` 仍为真时显示「正在收尾…」）。R30 落地后服务端不再存在「消息已可见但轮次未结束」的 6–15s 窗口，R31 因此退化为**兜底**：只对仍报 `is_processing=true` 的旧运行时 / 重放帧生效，保留不删。蒸馏侧的残余面（同 workspace 记忆索引并发写）记在 `16` 卡点决策表后的「R30 落地记录」，不在本页展开。

---

## 6. 依赖与风险

- **W2 审批**依赖后端解冻 `capabilities.approvals`，需安全评审——是第 1 层里唯一有后端改动的一项。
- **W8 通知/重连**依赖 SDK A2；A2 未完成时只能做到「提示断线」，无法自动恢复。
- **W12 技能管理**需协议增量，建议与 D2（Schema 与校验器）同批。
- **W14** 依赖 A3；不完成 A3 则 webui 事件层继续自建。
- **W1 与 W10 都触碰 composer 输入模型**（mention token 解析、附件 content），建议同一批做，避免两次重写输入层。

---

## 7. 现状对账（2026-09-09 代码核实）

### 表 1 · 协议已就绪、UI 未接

| 能力 | 后端 / SDK | webui | 归属 |
| --- | --- | --- | --- |
| `run/steer` | 已实现并 live 验证 | ✅ 已接（W3/R9：composer 引导形态 + 版本先读后用 + 冲突/终态处理） | W3 |
| 产物 `output_files` | `RunView` / `TurnResult` 已定义（`protocol.ts:204`、`turn-result.ts:32`）；webui 未跟踪 run receipt | 无展示 | W5 |
| `artifact/list` / `artifact/get` / `artifact.created` | ❌ **不存在**——`05` §8 有定义但标注延后实现，能力恒 `false` | 需协议增量 | 待 Artifact Phase |
| `approval/request` + `approval/respond` | 协议已定义 | ✅ 已接（W2/R8：`run/answer-decision` + 行内卡，能力位派生自 runtime） | W2 |
| `plan.created` / `plan.revised` | 事件已定义 | 无渲染 | W4 |
| `client.onNotification` | 已提供 | 未用 | W8 |
| `setMarketplaceAutoUpdate` / `importMarketplaceEntry` | 已提供 | 未接 | W13 |
| 结构化 `mentions` | 已存在（TC-INS-007） | 仅 `+` 菜单点选 | W1 |

### 表 2 · `11-webui-production-readiness.md` 清单核实

| 清单项 | 实际状态 | 归属 |
| --- | --- | --- |
| §1.1 正式认证与令牌管理 | ❌ 未做 | 方向四 · 待立项 |
| §1.2 Origin / CSP / CSRF | ❌ 未做 | 方向四 · 待立项 |
| §1.3 WS 断线重连与多标签协调 | 🟡 断线重连已做（T8 / W8）；**多标签协调按 D4=A 落在宿主面**（R13：每个标签各自订阅，只有逃出标签的副作用选主；SDK 不承担此项） | W8 ✅ |
| §2.1 消息重试 / 编辑 / 重新生成 | ❌ 未做 | W7 |
| §2.2 归档 / 回收站 | ❌ 未做 | 方向四 · 待立项 |
| §2.3 批量操作 | ❌ 未做 | 方向四 · 待立项 |
| §2.4 历史分页 / 虚拟列表 | 🟡 虚拟列表已用 `@tanstack/react-virtual`；游标分页有 `history-cursor.ts` | 已基本满足 |
| §3.1 Request ID / 错误上报 | ❌ 未做 | 方向四 · 待立项 |
| §3.2 健康检查 | ❌ 未做 | 方向四 · 待立项 |
| §3.3 配额 / 限流 | ❌ 未做 | 方向四 · 待立项 |
| §4.1 只读 / 不可访问标识 | ❌ 未做 | 方向四 · 待立项 |
| §4.2 重命名注册 | 🟡 会话重命名有；工作区重命名无 | 方向四 · 待立项 |
| §4.3 跨平台路径显示 | 🟡 `\\?\` 前缀剥离已做 | 已基本满足 |
| §5.1 token / 费用按 turn 汇总 | 🟡 仅 token 百分比，无费用 | W9 |
| §5.2 接近上限的压缩 / 新会话建议 | ❌ 未做 | W9 |
| §5.3 模型健康 / 能力限制 | ❌ 未做 | W9 |
| §6.1 正式 i18n | 🟡 `RunDetail` / `RunPanel` 硬编码英文 | W6 |
| §6.2 Markdown / XSS 安全渲染 | ✅ 未启用 `rehype-raw` | 已满足 |
| §6.3 附件上传 / 预览 | ❌ 未做 | W10 |
| §6.4 无障碍与 E2E 回归 | ❌ 无 E2E | 方向四 · 待立项 |

---

## 8. 与原计划的编号映射

| `16` 原编号 | 本文档 | 说明 |
| --- | --- | --- |
| B1 附件 / 图片输入 | W10 | — |
| B2 模型选择器收尾 | W9 | 与用量合并 |
| B3 消费 SDK 新能力 | W14 | 工程项 |
| B4 Slash 命令 | W1 | 合并进命令面板 |
| B5 `@` 提及补全 | W1 | 合并进命令面板 |
| B6 Sub-agent 可视化 | W4 + W6 | 拆为计划树与状态树 |
| B7 市场管理补全 | W13 | 非对齐项 |
| B8 设置 Dialog 补全 | W11 | — |
| B9 产物面板 | W5 | 扩展为产物与变更面板 |
| B10 `run/steer` | W3 | — |
| B11 通知与连接状态层 | W8 | — |
| B12 消息重试 / 编辑 / 重新生成 | W7 | — |
| — | W2 | 新增：审批卡 |
| — | W12 | 新增：技能 / 专家管理界面 |
| — | W1b | 新增：命令面板 V2（会话与模型动作，后置） |

---

## 9. 验收用例（TC-WEB / TC-SEC / TC-CATALOG）

> 本节由 `agent-store-v1-test-cases.md` 原 §8（Web/Flowy 与安全）与 §8.5（Skill / Connector 目录）并入（2026-09-11 文档合并）；TC 编号与用例正文保持不变，内部分节号沿用原文。

### 8. Web/Flowy 与安全

#### TC-WEB-001：Catalog 和兼容性状态

- 等级：P1
- 断言：UI 显示来源、版本、状态和 V1 能力；不把 manual-review/unsupported 显示为可运行

#### TC-WEB-002：Team Run 页面

- 等级：P1
- 断言：显示成员、Leader（规划角色）、Planning Context 摘要/digest、Plan/DAG、Step、Attempt、Timeline、Artifact；保留 Plan Revision；不显示 Planning Context 正文或成员完整 Prompt

#### TC-WEB-003：Approval UI

- 等级：P0
- 断言：显示脱敏资源和参数摘要；批准/拒绝由 App Server 二次校验；过期审批不能执行

#### TC-WEB-004：Artifact 路径边界

- 等级：P0
- 断言：不能通过 UI/API 读取任意本地路径；Artifact 必须绑定 run/workspace 并校验 digest

#### TC-SEC-001：敏感信息扫描

- 等级：P0
- 断言：日志、事件、Snapshot、Renderer 状态和公共响应不包含真实凭据；扫描结果无高危泄露

#### TC-SEC-002：高风险副作用审批

- 等级：P0
- 断言：发送、删除、发布、部署等操作没有有效 Approval 时返回 `approval_required` 或 `policy_denied`

#### TC-SEC-003：版权发布阻断

- 等级：P0
- 断言：`pending-legal-review` 资源不能进入公开 Marketplace 或默认安装包

### 8.5 Agent Store Skill / Connector 目录（新增切片）

#### TC-CATALOG-001：能力协商与目录方法

- 等级：P1
- 前置：生产装配注入 Skill/Connector/OAuth provider
- 操作：WS 初始化后调用 `skill/list`、`skill/get`、`connector/list`、`connector/get`、`connector/status`、`connector/test`、`connector/auth/start`、`connector/auth/status`、`connector/auth/logout`
- 断言：`initialize` 返回 `skills=true`、`connectors=true`、`oauth=true`；目录方法返回 Agent Store 公共形状（id/name/version/source/compatibility_status/enabled；Connector 含 kind/transport_summary/auth_mode/status），不含真实凭据、内部 ID 或文件系统绝对路径；未注入 provider 时对应方法返回 `unsupported_operation`

#### TC-CATALOG-002：Connector 状态合并规则

- 等级：P1
- 操作：对最近 Probe 失败的 Connector 调用 `connector/status`
- 断言：状态不得为 `connected`（对齐 TC-CONN-002）；OAuth 未就绪的 remote Connector 显示 `authorization_required`；Probe 成功后状态转为 `connected`（仅当无更高优先级失败状态）

#### TC-CATALOG-003：OAuth 状态透传

- 等级：P1
- 操作：`connector/auth/start` → 轮询 `connector/auth/status` → `connector/auth/logout` → 再次查询
- 断言：start 只返回状态与一次性授权 URL/会话（无 Token）；status 在 `not_authenticated`/`authenticated` 间翻转；logout 后回到 `not_authenticated`；stdio Connector 的 OAuth 方法返回稳定错误而非伪造成功

#### TC-CATALOG-004：Preset 绑定 Connector 的运行接线

- 等级：P1
- 操作：Preset 绑定非空 `mcp_server_ids` 时发起 `agent/run`
- 断言：引用的 Connector 均存在且启用时运行正常启动，attempt 会话冻结 `mcp_server_ids`（经 Conversation 层 `selected_mcp_server_ids` 校验/持久化路径）；存在缺失/禁用 Connector 时运行前返回 `connector_unavailable`，不启动 Run；`run/get`/`run/result` 公共视图不泄露凭据

#### TC-CATALOG-005：WebUI 目录视图（翻 `web/`）

- 等级：P1
- 操作：WebUI 经 WS 链路浏览「技能与连接器」目录
- 断言：导航入口仅在能力协商开启时显示；技能列表显示来源/版本/兼容性/所需连接器，详情只含公开指令摘要；连接器列表显示状态徽标且 `connected` 不伪报；OAuth 连接器可完成授权/取消授权交互，UI 不接触 Token

