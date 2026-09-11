# Agent Store 工具注入取舍（Tool Injection Policy）

> 状态：设计规格（2026-09-10）；**未实施**，本文不修改任何代码
> 前置：`00-architecture-decision.md`、`04-allo-runtime-adapter.md`、`16-sdk-webui-site-priority-plan.zh.md` §7 决策 3
> 口径：本文只回答"哪些工具进入 Store 会话、为什么、用哪种机制表达"；不定义公共协议，不替代测试总索引
> 术语：文中 **Store 会话** = App Server 创建的 Nomi 会话（当前 `create_app_server_nomi_chat`，以及后续 `team/run` 的 Leader 会话）

---

## 1. 结论摘要

1. **当前 Store 会话的工具面与需求反了**：Connector(MCP) 与绑定 Skill 被 `apply_app_server_chat_ceiling` 清掉，而桌面产品域工具（cron / meeting / computer / browser / knowledge / learning / media / companion / goal / requirement）全部保留。
2. **保留基线**（仅 6 项）：文件与执行族（workspace 受限）、`Skill`、Connector(MCP) 工具、`ToolSearch`、审批流、`update_plan`。
3. **必须关闭**：宿主控制类（`Computer`/`Browser`/`open`/`Lsp`）、产品域 sink 类、协作生命周期类（gateway `nomi_execution_*`）、gateway 其余能力、SSH 族。
4. **特例**：`nomi_delegate` 必须**保留**，因为它是 Team 的计划触发入口（`16` §7 决策 3）；但必须关闭 parallel-only 的 embedded 实现。
5. **表达方式**：用「不接线 + Config flag」表达，**不要用** `builtin_allowlist` 白名单（会连带滤掉 Connector 工具）。

---

## 2. 注入链路回顾（改动前必读）

Store 会话走的是与桌面会话同一条链路，逐层如下。

| 层 | 位置 | 职责 |
|---|---|---|
| L1 组装层 | `nomifun-ai-agent/src/factory/mod.rs` `AgentFactoryDeps` | 以 **sink / 配置对象**形式提供能力 |
| L2 工厂层 | `factory/nomi.rs::build` | authority 裁剪 + 能力解析，产出 `NomiResolvedConfig` + sink 参数 |
| L3 管理层 | `manager/nomi/agent.rs::new_with_search_provider` | 落成 `Config`：`allow_list` / `builtin_allowlist` / `mcp.servers` |
| L4 引导层 | `nomi-agent/src/bootstrap.rs::build` | 填充 `ToolRegistry`（内置族 + MCP 代理 + `retain_named`） |
| L5 动态层 | `manager/nomi/agent.rs` build 后 | `engine.registry_mut().register(...)` 补注册域工具 |
| L6 约束层 | 运行时 | 注册策略 / 审批名单 / `write_root` / coding boundary |

关键不变量：**能力即进程内对象**。`AgentFactoryDeps` 里的每个 `Option<Arc<dyn ...Sink>>` 为 `None` 就意味着"这个工具根本不会被创建"，不存在可被客户端 JSON 伪造的开关。

### 2.1 三个容易混淆的"名单"

| 名单 | 位置 | 语义 | 空值含义 |
|---|---|---|---|
| `config.tools.allow_list` | `nomi-config/src/config.rs` | **审批豁免**（免确认执行） | 用默认值 `["Read","Grep","Glob"]` |
| `config.tools.builtin_allowlist` | 同上 | **注册白名单**（是否存在于 registry） | 空 = 不限制 |
| `config.tools.skills.deny/allow` | 同上 | Skill 级权限，不是工具名单 | — |

---

## 3. 两类"延迟"必须先分清

"延迟注入"在本仓库指两件不同的事，混用会导致错误的关闭方案。

### 3.1 时序延迟（post-build 注册）

`AgentBootstrap::build()` **之后**才注册的工具。它们受 bootstrap 中已安装的 `retain_named` **持久策略**约束。

| 工具 | 注册点 | 条件 |
|---|---|---|
| `requirement_complete` / `requirement_update_status` | `agent.rs` build 后 | `requirement_sink` 为 `Some` |
| `recall_memories` / `save_memory` / `list_recent_events` | 同上 | companion 会话 |
| `recall_memories` / `propose_companion_memory` | 同上 | summon 已装载 |
| `companion_skill` / `create_companion_skill` | 同上 | companion 会话 |
| `knowledge_search` / `knowledge_read` | 同上 | 已挂知识库 |
| `learning_generate_course` / `learning_course_status` | 同上 | owner + 已挂库 |
| `knowledge_write` | 同上 | 回血开启 + 已挂库 |
| 媒体生成族 | `nomi_media::wire_flowy_media` | flowy media 配置就绪 |
| `update_goal` | `set_goal` / `set_goal_state` | 有 goal |
| `cron_create` / `cron_list` / `cron_delete` | `NomiAgentManager::register_cron_sink` | factory 层 `is_instance_owner` + 工厂存在 + owner_id |
| `meeting.*`（12 个） | `register_meeting_sink` | 同上 |

### 3.2 曝光延迟（`is_deferred()`）

工具已进 registry，但给 provider 的 `ToolDef.deferred = true`，模型只看到名字桩，需 `ToolSearch` 激活后才拿到完整 schema。

| 工具 | 来源 | 默认 |
|---|---|---|
| 全部 **MCP 代理工具**（Connector + gateway bridge） | `nomi-mcp/src/tool_proxy.rs` | **`deferred = true`**（`McpServerConfig::deferred` 缺省 true） |
| `EnterPlanMode` / `ExitPlanMode` | `nomi-agent/src/plan/tools.rs` | `true` |
| `nomi_delegate`（embedded 版） | `local_delegate_tool.rs` | `true` |

`ToolSearch` 自身永不 deferred（`nomi-tools/src/tool_search.rs` 注释明示）。

下列工具**刻意**显式声明 `is_deferred() == false`，因为其工具描述承载了工作流契约，deferred 桩会导致模型空参调用：`requirement_*`、`knowledge_search`/`knowledge_read`/`knowledge_write`、`learning_*`、`update_goal`。

> 结论：**`ToolSearch` 不可关闭**。MCP 默认延迟曝光，关掉它等于 Connector Catalog 全废。

### 3.3 交叉

只有 MCP 系工具可能同时命中两类延迟。cron / meeting 属于"时序延迟 + 立即曝光"，这也解释了为什么 `register_meeting_sink` 需要额外调 `engine.allow_named_tools(MEETING_TOOL_NAMES)` 补审批豁免，而 MCP 走的是 deferred 激活路径。

---

## 4. 取舍原则

任何工具进入 Store 会话前，必须同时满足：

1. **在 Store 领域模型里有对应物**（Agent / Skill / Connector / Team / Run / Artifact，见 `01-domain-model.md`）；
2. **不依赖 Desktop host 或产品 UI**（无客户端可响应的能力一律不注入）；
3. **不与 App Server 协议职责重叠**（生命周期、审批、状态查询归协议，不归模型工具）。

---

## 5. 逐项取舍表

图例：**保留** = Store 会话必须注入；**条件** = 由 Definition/Template 声明开启，默认关；**关闭** = 不接线。

### 5.1 保留（基线 6 项）

| 工具 | 依据 | 约束 |
|---|---|---|
| `Read` / `Write` / `Edit` / `ApplyPatch` | `04` §4.2 `[Workspace Policy]`；Artifact 落在 workspace | `write_root = workspace`，禁止越界（`04` §4.1 校验 #6） |
| `Bash` / `exec_command` / `write_stdin` | 工程型 Agent 的必需执行面 | 必须落在 `CapabilityPolicy.cwd_roots` 内。`roadmap` §2.2 的"任意 CLI/脚本执行"指宿主 Hook/脚本，不含 Agent 自身 shell |
| `Grep` / `Glob` / `DirTree` | 只读导航 | — |
| `Skill` | `roadmap` §2.1 Skill Catalog；`04` §4.2 `[Bound Skills]` | 只暴露 Definition 绑定的 Skill，不暴露宿主 auto-inject |
| **Connector(MCP) 工具** | `roadmap` §2.1 Connector Catalog；`04` §2.2「工具名必须经命名空间和策略过滤」 | 按 Connector 过滤 + 策略交集 |
| `ToolSearch` | MCP 默认 `deferred = true`，无它则 Connector schema 不可见 | 永不 deferred |
| 审批（`Confirmation` / `approval.required`） | `04` §5.1 事件表 | 非工具，但必须保留接线 |
| `update_plan` | 模型内生进度，无副作用、不写库 | 无条件注册，当前无 Config 开关 |

`nomi_delegate` 见 §7——它是本表之外的强制特例。

### 5.2 条件保留（默认关）

| 工具 | 默认 | 理由 |
|---|---|---|
| `WebSearch` / `WebExtract` | 关 | `04` §4.2 工具面是 Connector 驱动；联网应由 Connector 表达。仅当 Definition 未绑定 web connector 时才考虑开 |
| `remember` | 关 | V1 领域模型无跨 Run 记忆；Run/Attempt 持久化已独立。**注意：当前无法用 Config 关闭，见 §6.3** |
| `EnterPlanMode` / `ExitPlanMode` | 关 | `plan_gate` 是 Execution 级策略；Team 计划由服务端 Planner 产出，模型自驱 plan mode 会与 `plan.created` 语义冲突 |

### 5.3 关闭

| 工具 / 能力面 | 理由 |
|---|---|
| `Computer` | 宿主桌面控制；Store 不是 Desktop host |
| `Browser` + `open` bridge | 同上；依赖 `BrowserLaneClient` / Windows 启动器 |
| `Lsp` | `roadmap` §2.2 非目标「完整 LSP Runtime」；仅配置了 `lsp_servers` 时才注册 |
| `cron_*` | 产品域定时任务；调度归 App Server / Planner |
| `meeting.*` | 产品域会议能力；Store 无客户端可响应 |
| `companion_*` / summon | 产品域陪伴会话专属 |
| `knowledge_*` | 知识库不在 Agent/Skill/Connector 模型内 |
| `learning_*` | 课程生成，纯产品功能 |
| 媒体生成族 | 依赖 `GatewayConfig.media`，Store 无此配置 |
| `requirement_*` | AutoWork 任务板 |
| `update_goal` | goal 循环；Store 的续跑模型是 Attempt |
| gateway `nomi_execution_get` / `nomi_execution_update`（含 `request_user_decision`） | 与 App Server 协议职责重叠：`04` §5.1 已把审批建模为 Server Request；Team planned 由服务端 Planner 驱动 |
| gateway 其余能力（`nomi_list_conversations` 等） | 产品宿主桥，Store 用自己的协议 |
| SSH 工具族 | `ssh_host_id` 绑定是桌面产品能力 |

---

## 6. 关闭机制与坑

### 6.1 可用的四种机制

| 机制 | 适用工具 | 说明 |
|---|---|---|
| **Config flag** | `tools.web.enabled`、`tools.computer.enabled`、`tools.browser.enabled`、`plan.enabled` | 工厂在构造 `Config` 时置 `false` 即可 |
| **不接线 sink** | knowledge / learning / media / companion / summon / requirement / goal | factory 传 `None` → 工具不注册 |
| **factory `if` 守卫** | `cron_*`、`meeting.*` | 现有 `if is_instance_owner && let (Some(...), ...)` 增加 Store 判定 |
| **`install_embedded_agent_execution`** | embedded `nomi_delegate` | 置 `false` |
| **`gateway_mcp_config = None`** | gateway 全部能力 | Store 已有（`apply_app_server_chat_ceiling`） |

### 6.2 不要用 `builtin_allowlist` 白名单

bootstrap 的注册顺序是 MCP 代理在前、`retain_named` 在后：

```text
registry.register_mcp_tools(...)   // 注册 Connector 代理
...
registry.retain_named(&allowed_tools)  // 安装持久注册策略并裁剪
```

后果：白名单会把 **Connector 工具一起滤掉**，除非枚举 `mcp__<server>__<hash>` 这种 canonical 名——既不稳定也不可维护。

另外，`retain_named` 会**持久化注册策略**，之后所有动态注册（cron / meeting / 域 sink）都受它约束，且只收窄不放宽。`apply_model_only_ceiling` 用 `["update_plan"]` 正是因为"空列表 = 全放行"——这套机制是为"极小集合"设计的，不适合"保留大多数、去掉少数"。

### 6.3 当前无开关的工具（唯一硬缺口）

| 工具 | 注册点 | 现状 |
|---|---|---|
| `update_plan` | `bootstrap.rs`（其注释写明 "Always registered (not deferred)"） | 无条件，无 Config 开关 |
| `ToolSearch` | `bootstrap.rs` | 无条件 + `retain_named` 强制保留；**不应关闭** |
| `Skill` | `bootstrap.rs` | 无条件；Skill 目录来自 workspace 扫描 |
| `remember` | `bootstrap.rs`（`if let Some(mem_dir) = memory_dir`） | `auto_memory_dir` 实际恒为 `Some`，等同无条件 |

结论：若要关闭 `update_plan` / `remember`，需要给 `ToolsConfig` 增加一个**工具级 disable 列表**（与 `builtin_allowlist` 语义相反的减项），而不是复用白名单。`ToolsConfig` 目前**没有** denylist 字段。

---

## 7. Team 与 `nomi_delegate`（强制保留的特例）

依据 `16-sdk-webui-site-priority-plan.zh.md` §7 决策 3：Team Run 由 Leader 模型调用 `nomi_delegate(strategy=planned)` 触发。因此在 Store 会话里 **`nomi_delegate` 不是可选工具，而是 Team 的必需入口**。

但"保留 `nomi_delegate`"必须落到正确的实现上——仓库里有两个同名实现，能力完全不同：

| 实现 | 位置 | `strategy` | 持久化 | 适用 |
|---|---|---|---|---|
| Platform Gateway 版 | `nomifun-gateway/src/caps_agent_execution.rs`（`DelegateParams::Planned`） | `planned` / `parallel` | ✅ 走 `AgentExecutionEngine`，落库 Run/Event/Attempt | **Team 必须用这个形态** |
| embedded 版 | `nomi-agent/src/local_delegate_tool.rs` | **仅 `parallel`**（`ParallelDelegationStrategy` 只有一个变体） | ❌ 现场 `AgentExecutionId::new()`，`execute_fanout` 同步返回，无任何写入 | 仅适合 CLI/嵌入式宿主；**不得用于 Store / Team** |

> 注意：这不等于"Store 要放开 Platform Gateway"。`nomi_delegate(planned)` 目前**恰好**只被 gateway caps 模块实现，但 App Server 本身不依赖 gateway——正确做法是在进程内提供一个绑定 `AgentExecutionEngine` 的等价入口（例如 App Server 侧可注入的 delegate 能力），而不是把 gateway 的 135 项能力整体放开。

### 7.1 Store 会话当前的实际装配（错误组合）

| 装配项 | 当前值 | 应有值 |
|---|---|---|
| `gateway_mcp_config` | `None`（被 `apply_app_server_chat_ceiling` 清掉） | `None`（Store 不用 gateway） |
| `install_embedded_agent_execution` | `true`（`!has_platform_gateway && is_instance_owner`） | **`false`** |
| planned 入口 | **不存在** | 必须存在（in-process，绑 engine） |
| Connector(MCP) | **被清空** | 必须保留 |
| 绑定 Skill | **被清空（empty skill snapshot）** | 必须保留 |

即：模型拿到的是一个"只支持 parallel、且不落库"的壳，同时 Connector 与 Skill 都没了。

---

## 8. 待实施差距清单（本文不落地）

| # | 事项 | 涉及位置 | 关联 |
|---|---|---|---|
| 1 | 新增 `apply_agent_store_ceiling`：关 `Computer`/`Browser`/`open`/`Lsp`/web/plan-mode，不接 cron/meeting/knowledge/learning/media/companion/summon/requirement/goal 等 sink | `factory/nomi.rs` | §5、§6.1 |
| 2 | Store 会话置 `install_embedded_agent_execution = false` | `factory/nomi.rs` | §7.1 |
| 3 | 提供 in-process 的 planned delegate 入口（绑 `AgentExecutionEngine`），供 Leader 会话使用 | 待定：`nomifun-agent-execution` 或 App Server runtime wiring | `16` §7 决策 3 |
| 4 | 为 `ToolsConfig` 增加工具级 disable（至少覆盖 `remember`） | `nomi-config/src/config.rs` + `bootstrap.rs` | §6.3 |
| 5 | Store 会话保留 Connector 与绑定 Skill（不再走清空 skill/MCP 的 ceiling） | `nomifun-conversation` App Server 创建缝 | §5.1、§7.1 |
| 6 | `team/run` handler | `nomifun-app-server` | roadmap Phase 2/3 |

---

## 9. 验收关联

| 用例 | 关联点 |
|---|---|
| `TC-TEAM-001` | Leader Conversation 创建 + `execution_template_id` 绑定（§7） |
| `TC-TEAM-002` | Leader 经 `nomi_delegate(strategy=planned)` 触发；实现选择断言（§7） |
| `TC-RT-*` | 单 Agent Run 的工具面符合 §5.1 基线 |
| `TC-CONN-*` | Connector 工具确实进入 Store 会话（当前被 ceiling 清空的回归点） |
| `TC-SEC-*` | §5.3 关闭项不得出现在 Store 会话工具面 |

工具面的自动化断言建议以 **provider 可见工具名集合**为断言对象（`registry.tool_names()` / `to_tool_defs()`），而不是以配置字段为对象——配置正确但注册顺序变化仍可能改变实际工具面。

---

## 10. 与其他文档的关系

- 架构边界与决策总表：`00-architecture-decision.md`
- 领域模型（工具取舍的判定依据）：`01-domain-model.md`
- Runtime Adapter 与 Team 触发链：`04-allo-runtime-adapter.md`
- 公共契约（不改）：`10-public-contracts.md`
- 决策记录（本文依据）：`16-sdk-webui-site-priority-plan.zh.md` §7 决策 3
- 测试总索引：`agent-store-v1-test-cases.md`
