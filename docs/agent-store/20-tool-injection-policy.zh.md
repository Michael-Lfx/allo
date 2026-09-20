# Agent Store 工具注入策略（Tool Injection Policy）

> 状态：设计规格 + 落地记录（2026-09-11 重构并实施；Step 1–7 已落地，逐批记录见 §9.1／§9.2／§9.2.1／§9.2.2；**未覆盖项**亦在各批登记）
> 本次重构：① 表达层由「工厂硬编码 ceiling」改为「宿主策略文件 `~/.agent-store/config.toml [tools]` + 极薄会话求交」；② 补 §2「三份 config.toml 哪份对工具面生效」——这是全部取舍的前提；③ 逐项取舍表新增「表达层」列，区分「可配置」与「只能写在代码里」；④ 订正 5 处与代码不符的机制描述（见 §7.6 勘误表）；⑤ `[tools]` 的命名与匹配规则对齐参考实现 Kimi Code CLI（§7.8，含必须保留的差异）
> 参考实现：Kimi Code CLI 配置文件 §`tools` —— <https://www.kimi.com/code/docs/kimi-code-cli/configuration/config-files.html#tools>（`enabled` / `disabled`、MCP glob、三条 no-match 告警、执行前复核）。对齐点与差异见 §7.8
> 前置：`00-architecture-decision.md`、`01-domain-model.md`、`04-flowy-agent-store-runtime-adapter.md`、`16-sdk-webui-site-priority-plan.zh.md` §7 决策 3／决策 5、`21-open-decisions.zh.md` D3
> 口径：本文只回答「哪些工具进入 Store 会话、在哪一层表达、为什么」；不定义公共协议，不替代测试总索引
> 术语：文中 **Store 会话** = App Server 创建的 Nomi 会话（当前 `create_app_server_nomi_chat`，以及后续 `team/run` 的 Leader 会话）

---

## 1. 结论摘要

1. **表达层**：宿主策略走 `~/.agent-store/config.toml` 的 `[tools]`，**只做减项**；与引擎既有的 `%APPDATA%\nomi\config.toml`、`<workspace>/.nomi.toml` 取「更严者胜」（§2、§7.1）。
2. **保留基线**（6 项）：文件与执行族（workspace 受限）、`Skill`、Connector(MCP) 工具、`ToolSearch`、审批流、`update_plan`。
3. **必须关闭**：宿主控制类（`Computer`/`Browser`/`Lsp`）、产品域 sink 类、协作生命周期类（gateway `nomi_execution_*`）、gateway 其余能力、SSH 族（§6.3）。逐项给出表达层。
4. **不可配置项**：`nomi_delegate` 的 embedded／platform 实现选择**必须留在宿主组装里**——配置层被 vocabulary 门禁显式禁止表达（§7.5）。`nomi_delegate` 本身**必须保留**（Team 计划入口，`16` §7 决策 3），但必须关掉 parallel-only 的 embedded 实现。
5. **隔离前提**：本策略假定 **Store 以独立 host + 独立 data-dir 部署**。若与桌面/Web host 共用进程，宿主级策略会连带改变桌面会话工具面（§2.3）。
6. **表达方式**：用「宿主策略 + Config flag + 不接线」表达，**不用** `builtin_allowlist` 白名单（会连带滤掉 Connector 工具，且它是为「极小集合」设计的，§7.2）。

---

## 2. 前置：三份 config.toml，哪份对工具面生效

这一节是全部取舍的前提。仓库里同时存在三份配置文件，**只有一份既有产品写入路径、又能表达工具面**。

### 2.1 事实

| 文件 | 谁读 | 能控制工具面 | 证据 |
|---|---|---|---|
| `%APPDATA%\nomi\config.toml`（Linux `~/.config/nomi`） | Nomi 引擎 `Config::resolve` 的 global 段 | ✅ 原生 `[tools]` 就在这里生效 | `crates/agent/nomi-config/src/config.rs:960`（`global_config_path()`）、`:929-931`（`app_config_dir()`）、`:698-708`（`Config::resolve`） |
| `<会话 workspace>/.nomi.toml` | 同上，project 段（`cli.project_dir` = 会话 workspace，由 `manager/nomi/agent.rs:801` 注入） | ✅ **已通**，零代码即可裁剪 | `crates/agent/nomi-config/src/config.rs:703-708` |
| `~/.agent-store/config.toml` | 只有 `AgentStoreConfig` | ❌ **完全不通到引擎工具面**（今日只有 providers / memory / marketplace / import / credentials） | `crates/backend/nomifun-app-server/src/agent_store.rs:1`、`:36`、`:363-369` |

两个关键推论：

1. `global_config_path()` 只取 `dirs::config_dir()/nomi`，**既不看 `--data-dir` 也不看 host**。因此同一台机器上桌面 App 与独立 `agent-store serve` 共用同一份引擎全局 `[tools]`。
2. `~/.agent-store/config.toml` 是 Store 唯一「自己的」配置文件，且已带 `config/set` 写白名单（`agent_store.rs:444-465`、`nomifun-app-server/src/lib.rs:3599`），是三者中**唯一有产品化下发路径**的一份。

### 2.2 决策（`16` §7 决策 5）

`[tools]` 挂在 `~/.agent-store/config.toml`，理由：

- 与 `21` D3=B 已拍板的 `[approvals]`（「策略由 `~/.agent-store/config.toml` 的 `[approvals]` 声明」）**同一份文件、同一语义层**：宿主策略归宿主配置文件，`[tools]` 与 `[approvals]` 是兄弟段。
- 只有它同时具备「表达宿主策略」的语义与「被产品写入/回读」的面（读面 `config_view`：`nomifun-app-server/src/lib.rs:3542`；写面 `AgentStoreConfigPatch`：`agent_store.rs:457`）。
- 引擎全局那份没有产品写入面，只能手改，且与桌面会话共享；`<workspace>/.nomi.toml` 已通但配置散落在每个会话目录，不可集中管理——两者都保留为**可叠加的更严项**，不作为权威来源。

### 2.3 隔离前提与残留的坑

本策略的隔离性来自**部署形态**，不来自机制：

- `apps/agent-store/src/main.rs:267-270` 指向 `~/.agent-store/config.toml` —— 独立 host，策略即 Store 策略，无隔离问题。
- **但 `apps/web/src/main.rs:247-252` 指向同一份文件**，且 `nomifun-app/src/router/routes.rs:1138` 在两个 host 里都挂载了 `app_server_routes`。所以 `[tools]` 必须**只由 agent-store host 消费**，需要一枚宿主位（建议落在 `Cli`/`AppConfig`，由 `apps/agent-store` 置位；其他 host 读到该表也不采纳）。这是本路线唯一的残留风险点。

---

## 3. 注入链路回顾

Store 会话走的是与桌面会话同一条链路，逐层如下。

| 层 | 位置 | 职责 |
|---|---|---|
| L1 组装层 | `nomifun-ai-agent/src/factory/mod.rs:115` `AgentFactoryDeps` | 以 **sink / 配置对象**形式提供能力；唯一生产构造点 `nomifun-app/src/services.rs:2948` |
| L2 工厂层 | `factory/nomi.rs::build` | authority 裁剪 + 能力解析，产出 `NomiResolvedConfig` + sink 参数 |
| L3 管理层 | `manager/nomi/agent.rs::new_with_search_provider` | `Config::resolve`（**这里才读 `[tools]`**）落成 `Config`：`allow_list` / `builtin_allowlist` / `mcp.servers` |
| L4 引导层 | `nomi-agent/src/bootstrap.rs::build` | 填充 `ToolRegistry`（内置族 + MCP 代理 + `retain_named`） |
| L5 动态层 | `manager/nomi/agent.rs` build 后 | `engine.registry_mut().register(...)` 补注册域工具 |
| L6 约束层 | 运行时 | 注册策略 / 审批名单 / `write_root` / coding boundary |

关键不变量：**能力即进程内对象**。`AgentFactoryDeps` 里的每个 `Option<Arc<dyn ...Sink>>` 为 `None` 就意味着「这个工具根本不会被创建」，不存在可被客户端 JSON 伪造的开关。

> **L3 是本次重构的关键定位**：`web.enabled` / `plan.enabled` / `lsp_servers` 由 L3 的 `Config::resolve` 决定，**不在 L2**。任何「在工厂里置 false」的写法都落不到这几个开关上（见 §7.6 勘误 ①）。

### 3.1 四份「名单」

| 名单 | 位置 | 语义 | 空值含义 |
|---|---|---|---|
| `config.tools.allow_list` | `nomi-config/src/config.rs` | **审批豁免**（免确认执行） | 用默认值 `["Read","Grep","Glob"]` |
| `config.tools.builtin_allowlist` | 同上（`:398`） | **注册白名单**（是否存在于 registry） | 空 = 不限制 |
| `config.tools.builtin_denylist` | **本文新增**（同结构） | **注册黑名单**（减项，对 bootstrap 之后注册同样生效） | 空 = 不排除 |
| `config.tools.skills.deny/allow` | `config.rs:337-342` | Skill 级权限，不是工具名单 | — |

生效集定义：`(allowlist 为空 ? 全放行 : allowlist) ∧ ¬denylist`。两者正交，必须同时保留。

---

## 4. 两类「延迟」必须先分清

「延迟注入」在本仓库指两件不同的事，混用会导致错误的关闭方案。本节结论与关闭机制无关，故保持原样。

### 4.1 时序延迟（post-build 注册）

`AgentBootstrap::build()` **之后**才注册的工具。它们受 bootstrap 中已安装的 `retain_named` **持久策略**约束（`registry.rs:656`）。

| 工具 | 注册点 | 条件 |
|---|---|---|
| `requirement_complete` / `requirement_update_status` | `agent.rs` build 后 | `requirement_sink` 为 `Some` |
| `recall_memories` / `save_memory` / `list_recent_events` | 同上 | companion 会话 |
| `recall_memories` / `propose_companion_memory` | 同上 | summon 已装载 |
| `companion_skill` / `create_companion_skill` | 同上 | companion 会话 |
| `knowledge_search` / `knowledge_read` | 同上 | 已挂知识库 |
| `learning_generate_course` / `learning_course_status` | 同上 | owner + 已挂库 |
| `knowledge_write` | 同上 | 回血开启 + 已挂库 |
| 媒体生成族 | `nomi_media::wire_flowy_media`（`manager/nomi/agent.rs:1298`） | 宿主 `config.toml` 的 `[media]` 就绪 |
| `update_goal` | `set_goal` / `set_goal_state` | 有 goal |
| `cron_create` / `cron_list` / `cron_delete` | `NomiAgentManager::register_cron_sink`（`agent.rs:2638`） | factory 层 `is_instance_owner` + 工厂存在 + owner_id |
| `meeting.*`（12 个） | `register_meeting_sink`（`agent.rs:2649`） | 同上 |

### 4.2 曝光延迟（`is_deferred()`）

工具已进 registry，但给 provider 的 `ToolDef.deferred = true`，模型只看到名字桩，需 `ToolSearch` 激活后才拿到完整 schema。

| 工具 | 来源 | 默认 |
|---|---|---|
| 全部 **MCP 代理工具**（Connector + gateway bridge） | `nomi-mcp/src/tool_proxy.rs` | **`deferred = true`**（`McpServerConfig::deferred` 缺省 true） |
| `EnterPlanMode` / `ExitPlanMode` | `nomi-agent/src/plan/tools.rs` | `true` |
| `nomi_delegate`（embedded 版） | `local_delegate_tool.rs` | `true` |

`ToolSearch` 自身永不 deferred（`nomi-tools/src/tool_search.rs` 注释明示）。

下列工具**刻意**显式声明 `is_deferred() == false`，因为其工具描述承载了工作流契约，deferred 桩会导致模型空参调用：`requirement_*`、`knowledge_search`/`knowledge_read`/`knowledge_write`、`learning_*`、`update_goal`。

> 结论：**`ToolSearch` 不可关闭**。MCP 默认延迟曝光，关掉它等于 Connector Catalog 全废。

### 4.3 交叉

只有 MCP 系工具可能同时命中两类延迟。cron / meeting 属于「时序延迟 + 立即曝光」，这也解释了为什么 `register_meeting_sink` 需要额外调 `engine.allow_named_tools(MEETING_TOOL_NAMES)` 补审批豁免，而 MCP 走的是 deferred 激活路径。

---

## 5. 取舍原则

任何工具进入 Store 会话前，必须同时满足：

1. **在 Store 领域模型里有对应物**（Agent / Skill / Connector / Team / Run / Artifact，见 `01-domain-model.md`）；
2. **不依赖 Desktop host 或产品 UI**（无客户端可响应的能力一律不注入）；
3. **不与 App Server 协议职责重叠**（生命周期、审批、状态查询归协议，不归模型工具）。

判别新增：**表达层优先于机制**。能由宿主策略表达的，不要写成代码常量；只能由宿主组装决定的（如执行部署形态），不要试图做成配置——配置层看不懂它，也不该懂（§7.5）。**叠加只做「更严者胜」**：任何一层说关就是关，不存在「配置说开、Store 说关、结果开了」的路径。

---

## 6. 逐项取舍表

图例——**保留** = Store 会话必须注入；**条件** = 由 Definition/Template 声明开启，默认关；**关闭** = 不接线。
「表达层」列：`host-config` = `~/.agent-store/config.toml [tools]`；`session` = 会话快照/求交；`code` = 宿主组装，不可配置。

### 6.1 保留（基线）

| 工具 | 依据 | 约束 | 表达层 |
|---|---|---|---|
| `Read` / `Write` / `Edit` / `ApplyPatch` | `04` §3.2 `[Workspace Policy]`；Artifact 落在 workspace | `write_root = workspace`，禁止越界（`04` §3.1 校验 #6） | `code`（无条件注册，靠 `write_root` 钳制） |
| `Bash` / `exec_command` / `write_stdin` | 工程型 Agent 的必需执行面 | 必须落在 `CapabilityPolicy.cwd_roots` 内。`roadmap` §2.2 的「任意 CLI/脚本执行」指宿主 Hook/脚本，不含 Agent 自身 shell | `code` |
| `Grep` / `Glob` / `DirTree` | 只读导航 | — | `code` |
| `Skill` | `roadmap` §2.1 Skill Catalog；`04` §3.2 `[Bound Skills]` | 只暴露 Definition 绑定的 Skill，不暴露宿主 auto-inject | `session`（`extra.skills` 快照，§7.4） |
| **Connector(MCP) 工具** | `roadmap` §2.1 Connector Catalog；`04` §3.1 校验 #4「Connector refs 已安装，工具策略可解析」 | 只暴露 Definition 绑定的 Connector（显式 id 栅栏，§7.3） | `session` |
| `ToolSearch` | MCP 默认 `deferred = true`，无它则 Connector schema 不可见 | 永不 deferred；**不可关闭** | `code` |
| 审批（`Confirmation` / `approval.required`） | `04` §5.1 统一事件；`05` §7「Approval 与 Server Request」；`21` D3=B | 非工具，但必须保留接线；策略归 `[approvals]`（未实施） | `host-config`（`[approvals]`） |
| `update_plan` | 模型内生进度，无副作用、不写库 | 无条件注册（`bootstrap.rs:1117-1119`），今日无 Config 开关 | `code` + `[tools].deny` 可关（§7.4） |

`nomi_delegate` 见 §8——它是本表之外的强制特例。

### 6.2 条件保留（默认关）

| 工具 | 默认 | 表达层 | 理由 |
|---|---|---|---|
| `WebSearch` / `WebExtract` | 关 | `[tools].web = false` | `04` §3.2 工具面是 Connector 驱动；联网应由 Connector 表达。仅当 Definition 未绑定 web connector 时才考虑开 |
| `remember` | 关 | `[tools].deny = ["remember"]`；或整体的 `[memory].enabled = false` | V1 领域模型无跨 Run 记忆；Run/Attempt 持久化已独立。**订正（2026-09-20）**：原文写「当前无任何开关，必须靠新增的减项关闭」——现在 `remember` 所在的**整个内置记忆子系统**已有一个宿主级总开关 `[memory].enabled`（`33-memory-master-switch.zh.md`），它同时停掉提示词段落、`remember`、轮后蒸馏与引用回写四面；`[tools].deny` 仍可用于**只**摘掉这一个工具而保留另外三面 |
| `EnterPlanMode` / `ExitPlanMode` | 关 | `[tools].plan = false` | `plan_gate` 是 Execution 级策略；Team 计划由服务端 Planner 产出，模型自驱 plan mode 会与 `plan.created` 语义冲突 |

### 6.3 关闭

| 工具 / 能力面 | 表达层 | 理由 |
|---|---|---|
| `Computer` | `[tools].computer = false` | 宿主桌面控制；Store 不是 Desktop host |
| `Browser` | `[tools].browser = false` | 同上；依赖 `BrowserLaneClient` |
| `open` bridge | —（不适用） | 它是 ACP CLI 的 Windows 专用 stdio MCP（`nomifun-app/src/commands/open_stdio.rs:1-16`），Nomi Store 会话的工具面里本就不存在；此处仅作防御性登记 |
| `Lsp` | `[tools].lsp = false` | `roadmap` §2.2 非目标「完整 LSP Runtime」；仅 `lsp_servers` 非空时注册（`bootstrap.rs:700-715`） |
| `cron_*` | `[tools.domains].cron = false` | 产品域定时任务；调度归 App Server / Planner |
| `meeting.*` | `[tools.domains].meeting = false` | 产品域会议能力；Store 无客户端可响应 |
| `companion_*` / summon | `[tools.domains].companion = false` | 产品域陪伴会话专属 |
| `knowledge_*` | `[tools.domains].knowledge = false` | 知识库不在 Agent/Skill/Connector 模型内 |
| `learning_*` | `[tools.domains].learning = false` | 课程生成，纯产品功能 |
| 媒体生成族 | `[tools.domains].media = false` | 依赖宿主 `config.toml` 的 `[media]`；Store 无此配置 |
| `requirement_*` | `[tools.domains].requirement = false` | AutoWork 任务板 |
| `update_goal` | `[tools.domains].goal = false` | goal 循环；Store 的续跑模型是 Attempt |
| gateway `nomi_execution_get` / `nomi_execution_update`（含 `request_user_decision`） | `code` | 与 App Server 协议职责重叠：审批已建模为 Server Request（`05` §7；`10`「`approval/request` = Server Request」）；Team planned 由服务端 Planner 驱动 |
| gateway 其余能力（`nomi_list_conversations` 等） | `code` | 产品宿主桥，Store 用自己的协议 |
| SSH 工具族 | `code` | `ssh_host_id` 绑定是桌面产品能力；Store 会话的 `extra` 由 create seam 生成，不含该键（`service.rs:5924` 更新面亦不可写入） |

---

## 7. 表达机制与坑

### 7.1 五种机制

| 机制 | 适用工具 | 说明 |
|---|---|---|
| **宿主策略文件** `[tools]` | enabled / disabled / web / computer / browser / plan / lsp / domains | 只做减项；与引擎全局 `<project>/.nomi.toml` 求「更严者胜」（§2）。`enabled`/`disabled` 的命名与匹配规则对齐参考实现（§7.8） |
| **会话快照/求交** | Connector、绑定 Skill | `extra.skills`、`selected_mcp_server_ids`（§7.3、§7.4） |
| **不接线 sink** | knowledge / learning / media / companion / summon / requirement / goal | factory 传 `None` → 工具不注册 |
| **factory `if` 守卫** | `cron_*`、`meeting.*` | 现有 `if is_instance_owner && let (Some(...), ...)` 增加 domains 判定 |
| **宿主组装（不可配置）** | embedded `nomi_delegate`、`write_root`、gateway 面 | 由 `AgentFactoryDeps` / authority 决定，配置层不表达（§7.5） |

推荐形态（扁平布尔，不复用引擎的 `[tools.computer]` 嵌套结构——那些带 `max_screenshot_edge` 之类引擎细节，不该出现在 Store 策略里）：

```toml
[tools]
# 允许列表：非空时仅列出的工具可用；空数组或缺省 = 不约束（对应引擎 builtin_allowlist）
enabled  = []
# 禁止列表：在 enabled 之后应用，天然只收窄；对 bootstrap 之后动态注册的工具同样生效
disabled = ["remember", "mcp__notion__*"]

# 单项开关
web = false
computer = false
browser = false
plan = false
lsp = false

[tools.domains]       # 缺省全 true = 今日行为
cron = false
meeting = false
knowledge = false
learning = false
media = false
companion = false
requirement = false
goal = false
```

另有**进程级覆盖**：环境变量 `AGENT_STORE_TOOLS`（JSON，整份替换上面的 `[tools]` 表，供自己 spawn 宿主的调用方/CI 使用）——见 §7.10。

与引擎字段的落点映射（改错层就会失效，见 §7.6 ①②③）：

| Store 策略 | 引擎落点 | 落在哪一层 |
|---|---|---|
| `[tools].enabled` 非空 | `config.tools.builtin_allowlist` | L3 manager |
| `[tools].disabled` | **新增** `config.tools.builtin_denylist` → `registry.deny_named()` | L3 灌入 + L4 生效 |
| `web` / `plan` / `lsp` | `config.tools.web.enabled` / `config.plan.enabled` / `config.tools.lsp_servers` | L3 manager |
| `computer` / `browser` | `NomiBuildExtra.computer_use` / `browser_use = Some(false)` | L2 factory |
| `[tools.domains]` | sink 接线与 cron/meeting 守卫（media 例外，在 manager） | L2 factory + L3 manager |

### 7.2 必须有 `disabled` 减项，而不是复用白名单

bootstrap 的注册顺序是 MCP 代理在前、`retain_named` 在后：

```text
registry.register_mcp_tools(...)   // 注册 Connector 代理（bootstrap.rs:1129）
...
registry.retain_named(&allowed_tools)  // 安装持久注册策略并裁剪（bootstrap.rs:1145）
```

白名单有三个问题，前两个是「能用但别扭」，第三个是致命的：

1. **它会连带滤掉 Connector 代理工具**：未列名的 MCP 代理会被同样裁掉，调用方必须知道 canonical 命名规则才能表达。借助 glob（`mcp__<server>__*`，§7.7）可以解决，但这把「知道内部命名」变成了使用前提。
2. **它的语义是为「极小集合」设计的**：空 = 全放行，所以只能表达「只要这几个」，不能表达「除了这几个都要」。`apply_model_only_ceiling` 用 `["update_plan"]` 正是这一语义的写照。
3. **它表达不了本策略的实际需求**——「保留大多数、去掉少数」：目标集合是「基线若干项 + 全部绑定 Connector + 全部绑定 Skill + 上游未来新增项」，用白名单表达要逐一枚举，任何上游新增工具都会默认消失，且没有编译期或启动期信号。

因此新增与它**正交**的 `disabled` 减项，两者叠加：

```text
生效集 = (enabled 为空 ? 全放行 : enabled) ∧ ¬disabled
```

实现要点：`ToolRegistry` 持有一个持久 `disabled` 集合，`registration_policy_allows`（`registry.rs:407`）改为 `¬disabled ∧ policy.allows`，`retain_named` 既有的「只收窄、对后续注册生效」语义原样保留（`registry.rs:656`）——这样 post-build 注册的 cron / meeting / 域 sink 也绕不过同一个减项。

### 7.3 Connector 必须走显式 id 栅栏

`selected_mcp_server_ids` 为 `None` 的语义是**绑定全部 enabled 的非 builtin MCP server**（`service.rs:5360-5381`、`factory/nomi.rs:1658-1687`）。而 `create_app_server_nomi_chat`（`service.rs:4845`）今日根本不传这个键。

所以「保留 Connector」**不能**靠删除 factory 的 `!is_app_server_chat` 守卫（`factory/nomi.rs:327`、`:339`）来实现——那会把宿主所有 MCP 泄进 Store 会话。正确做法：create seam 接受 Definition 绑定的 Connector id 列表，写 `selected_mcp_server_ids = Some(ids)`（**空数组也是硬栅栏**），再放守卫。**顺序上必须先落栅栏**。

### 7.4 Skill 绑定发生在 conversation seam，不在 factory

Skill 快照在 `create_inner` 冻结：`compute_initial_skills(auto_inject, preset_enabled, exclude_auto_inject)`，公式 `(auto_inject − exclude) ∪ preset_enabled`（`skill_snapshot.rs:9-21`）。App Server create 今日 `preset_id = None` 且全量 `exclude_auto_inject_skills` → `extra.skills = []`。

所以「保留绑定 Skill」= 让 create seam 接受绑定名单并写 `preset_enabled_skills`（`preset` 优先级高于 `exclude`，见 `skill_snapshot.rs:17-19`）。factory 侧没有「清空 skill」的 ceiling，无需改。

### 7.5 无开关项与不可配置项

| 工具 | 现状 | 处置 |
|---|---|---|
| `update_plan` | `bootstrap.rs:1117-1119` 无条件注册，无 Config 开关 | `[tools].disabled` |
| `ToolSearch` | 无条件 + `retain_named` 强制保留 | **不应关闭** |
| `Skill` | 无条件；目录来自 workspace 扫描 | 由快照控制暴露面（§7.4） |
| `remember` | `bootstrap.rs:768-770`（`if let Some(mem_dir)`）。**订正（2026-09-20）**：原文写「`memory_dir` 实际恒为 `Some`」，现已有第二个 `None` 来源——宿主可在 `~/.agent-store/config.toml` 里写 `[memory].enabled = false` 把整个内置记忆关掉（`33-memory-master-switch.zh.md`）；此时该工具**根本不注册**，因此它也**不在** `[tools]` 减项的管辖范围内（不存在的工具无从匹配，见 §7.7 的 no-match 告警语义） | `[tools].disabled`，或宿主级 `[memory].enabled` |
| embedded `nomi_delegate` | `bootstrap.rs:952-954`，由 `install_embedded_agent_execution`（`:915`）决定 | **不可配置**，见下 |

**为什么 embedded vs platform 不能做成配置**：`scripts/check-agent-vocabulary.mjs:342-358` 扫描 `ToolsConfig` 结构体块，禁止出现 `install_embedded_agent_execution` / `in_process_delegation` / `in_process_spawn` / `delegation_execution` 一类标识符；`nomi-config/src/config.rs:975-979` 还会把这三个历史键从配置文件里主动删除。执行部署形态是**嵌入宿主的决定**，不是用户配置。任何把它做成 `[tools]` 开关的方案都会同时踩到门禁与历史迁移。

### 7.6 勘误（相对本文 2026-09-10 版）

| # | 原表述 | 代码事实 | 影响 |
|---|---|---|---|
| ① | §6.1「工厂在构造 `Config` 时置 `false`」即可关 `tools.web.enabled` / `plan.enabled` | 工厂**不构造** `Config`；`Config::resolve` 在 `manager/nomi/agent.rs:804`，per-session 覆盖目前只对 computer/browser 做了（`:871-876`） | 改法必须落在 **L3 manager**，不是 L2 工厂 |
| ② | §5.3 `Lsp`「仅配置了 `lsp_servers` 时才注册」 | 正确，但 `lsp_servers` 是宿主 `config.toml` 值（`config.rs:381`），**无会话开关** | 同 ①，需会话级覆盖 |
| ③ | §6.1 媒体族用「不接线 sink」表达 | 媒体族不是 sink：`wire_flowy_media(registry, &gateway_config, …)` 在 `agent.rs:1298` **无条件**执行 | 须在 manager 加 domains 判定 |
| ④ | §8#5「Store 会话保留 Connector」 | `None` = 全部 enabled MCP（§7.3） | 必须同时落显式 id 栅栏，否则泄露宿主全部 MCP |
| ⑤ | §7.1「绑定 Skill 被清空（empty skill snapshot）」 | 清空在 conversation seam（§7.4），**不在** factory ceiling | 「保留 Skill」的改动点在 `service.rs`，不在 `factory/nomi.rs` |

另有两处「防御性过度」已在本版归位：`open` bridge 不是 Nomi Store 会话的工具面（§6.3）；gateway 全关已达成（`factory/nomi.rs:310` 的 `platform_gateway_entitled` 已排除 app-server chat，`:175` 无条件置 `None`）。

### 7.7 命名匹配与告警规则

`enabled` / `disabled` 的条目如何匹配工具名：

| 工具类别 | 匹配方式 | 例 |
|---|---|---|
| 内置工具 | **大小写敏感的精确匹配** | `Read`、`remember`、`update_plan` |
| MCP 代理工具（Connector） | glob，且**只有 `mcp__` 命名空间下的通配符有意义** | `mcp__github__*` |

我们的 canonical MCP 名形状（`nomi-mcp/src/tool_proxy.rs:27-36`、`:447-458`）：

```text
mcp__ {slug ≤ 42} __ {base32(sha256) 前 16 字符}      总长 ≤ 64
slug = sanitize("{server_name}__{tool_name}") 截断
```

因为 slug 保留了 `{server}__` 前缀，`mcp__<server>__*` 是稳定可用的整服务器 glob——前提是 server 名本身没被截断吃掉（server 名 ≲ 40 字符时安全）。这也说明：`disabled` 能表达「禁掉整个 Connector」，而 §7.3 的 id 栅栏仍然是**另一件事**——栅栏决定「绑定了哪些 Connector」，`disabled` 决定「已绑定的里再关掉哪些」，两者不可互相替代。

**三条「匹配不到任何工具」的写法必须在启动时告警**（照搬参考实现，§7.8）：

1. `mcp__` 命名空间之外使用通配符：`enabled = ["*"]` 会关掉所有工具，`disabled = ["*"]` 什么也禁不掉——两者都不是字面直觉；
2. 缺少工具段的 `mcp__` 字面量（如 `mcp__github`）：表达整个 server 必须写 `mcp__github__*`；
3. 任何已注册或内置工具都没有的名字（含大小写不匹配）。

**复用现成匹配器**：`nomi-config/src/hooks.rs:240` 的 `glob_match`（基于 `glob::Pattern`）已经在服务 hook 的 `tool_match`。实施时应把它提到共享位置供 `RegistrationPolicy` 复用，不要再写第二个 glob 实现。

### 7.8 参考实现（Kimi Code CLI）

本节的 `[tools]` 形态不是自创，与 Kimi Code CLI 的配置文件 §`tools` 对齐：<https://www.kimi.com/code/docs/kimi-code-cli/configuration/config-files.html#tools>。该实现了「全局工具开关」这一层，其语义要点：

- `enabled`（全局允许列表，非空时仅列出的工具可用，省略或空数组均表示不约束）与 `disabled`（全局禁止列表，**在 `enabled` 之后应用**）；
- **对所有会话中的每个 Agent 生效，并在 Agent 自身的 `tools` / `disallowedTools` 策略之上再取一次交集**；
- 内置工具按名称精确匹配，MCP 工具用 glob；
- 三条 no-match 告警（已在 §7.7 采纳）；
- 该节「不仅决定模型能看到哪些工具，还会在执行前再次强制检查」；`[permission]` 是**独立**的一层，决定哪些操作需要审批。

**一致点**：全局 ∩ Agent 级两层结构（对应我们的宿主策略 ∩ 会话/Definition 绑定）；`enabled` 空 = 不约束；可用性（`[tools]`）与审批（我们对应 `21` D3 的 `[approvals]`）分层，与 §3.1 的四份名单同构。

**必须保留的差异**（照抄会错的地方）：

1. **多一层产品域开关 `[tools.domains]`**。参考实现没有 cron / meeting / knowledge / learning / media / companion / requirement / goal 这类产品域 sink 工具，其工具面完全可由名字列表表达。我们不行：这些工具在 sink 不接线时**根本不存在**，把它们的名字写进 `disabled` 会被 §7.7 规则 3 判为无效条目；反之把它们写进 `enabled` 也只是空转。所以产品域必须用独立开关在接线处决定。
2. **enforcement 点是注册期移除，比参考实现更强**：被 `disabled` 命中的工具不进 `ToolRegistry`（`registry.rs:656`/新增 `deny_named`），因此既不可见也不可调用，不存在「绕过广告过滤直接调用」的路径；代价是策略必须对 **post-build 动态注册**同样生效，这正是 §7.2 要求 `disabled` 具有持久集合语义的原因。
3. **不引入第三种 Agent 级写法**：参考实现允许在 Agent 文件里写 `tools` / `disallowedTools`；我们的对应物是 Definition/Template 绑定（会话 `allowed_tools` + Connector/Skill 快照），不新增平行机制。

---

### 7.9 参考实现（Kimi Code CLI）：`mcp.json` 声明 MCP

§7.8 是**能不能用**这一层（工具面的收窄）；MCP server **从哪来**是另一层。参考实现把它放在 `mcp.json`：<https://www.kimi.com/code/docs/kimi-code-cli/customization/mcp.html>

- 两层文件：`~/.kimi-code/mcp.json`（跨项目）+ 工作目录下的 `.kimi-code/mcp.json`（只对当前仓库），同名条目**项目级覆盖用户级**；
- `mcpServers.<name>`：含 `command` → stdio；含 `url` 且未写 `transport` → HTTP；`transport:"sse"` → 旧式 SSE；
- 可选字段：`env` / `cwd`（stdio）、`headers` / `bearerTokenEnvVar`（HTTP·SSE）、`enabled`、`startupTimeoutMs`、`toolTimeoutMs`、`enabledTools`、`disabledTools`；
- 工具名 `mcp__<server>__<tool>`，权限规则用 `*` / `**` 通配（与 §7.8 同一命名空间）；
- **改动只对新会话生效**：编辑或新增的 server 不注册进已打开的会话；从配置里删掉的 server 在已开会话中显示 `removed`，工具仍可见但调用失败；
- 项目级 stdio 条目会在会话启动时执行本地命令，因此有工作区信任提示。

**读/写面（`21` D17，2026-09-17 写 · 2026-09-18 补读）**：用户级
`~/.agent-store/mcp.json` 现在可由宿主**读回原文**并**写**——`config/get-mcp`（编辑器专用；
它是唯一返回原文的读面，`config/get.mcp` 那份 verdict 视图仍不含任何 `env` / `headers` 取值）、
`config/set-mcp`（全文，写前用 §7.9.1 的同一个解析器验，不过则零写入）与
`config/set-mcp-enabled`（只改一条目的 `enabled`，文本级最小编辑）。规格与防线见
`05` §4.10；本文件的 §7.9.1 就是那道校验的规格，写面**不引入第二套规则**。

**Agent Store 的对齐与差异**（决策见 `21` D14）：

| 维度 | 对齐 | 必须保留的差异 |
|---|---|---|
| 文件与 schema | `~/.agent-store/mcp.json`，`mcpServers` 同名同形，三类传输判定规则一致 | **只做用户级**：项目级整体不做（Store 是常驻服务端、无交互式信任面，`21` D14 ③=C） |
| 可选字段 | 本文成文时参考实现文档化的九个字段**全部支持**（`env` / `cwd` / `headers` / `bearerTokenEnvVar` / `enabled` / `startupTimeoutMs` / `toolTimeoutMs` / `enabledTools` / `disabledTools`，落点见 §7.9.1） | **唯一的有界差异**：两个超时参考实现允许到 `2147483647` ms，我们按引擎自己的上界卡在 `≤ 600000` ms 并**拒绝**越界值（不夹取、不静默改小）；**未知字段**仍**拒绝该条目**并点名——静默忽略会改变用户声明的安全语义（`enabledTools` 被忽略 = 用户以为排除掉的工具仍可调用）。参考实现成文之后新增的第十个字段 `deferred` **不在支持范围内**，它正是靠这条规则被拒，理由与落地前提见 §7.9.4 |
| 工具命名与权限 | 同一 `mcp__` 命名空间；server 级 `enabledTools` / `disabledTools` 在**注册之前**裁剪（§7.9.2），宿主 `[tools]` 仍在最后求交 | 不引入第三种工具级写法：整组关闭既可以写在声明里（`disabledTools: ["mcp__<key>__*"]`），也可以写在宿主 `[tools] disabled = ["mcp__<key>__*"]`——后者是全局策略，前者只作用于本 server |
| 生效时机 | **比参考实现更严格**：宿主**启动时读一次**（`resolve_host_mcp_declarations` 是全仓唯一调用点，`nomifun-app/src/services.rs`），落内存后随 factory deps 传入每个会话；因此新增 / 编辑 / 删除都要**重启宿主**才生效，UI 里的编辑与开关也一样（2026-09-19 订正：本文原写「每会话构建时读盘」，与实现不符） | 没有 `removed` 墓碑态：删除后重启即彻底不可见，已开会话更没有可见标记（登记为未做）；参考实现的「改动只对新会话生效」在这里是更严的一档 |
| 来源优先级 | 参考实现是 项目级 > 用户级 | 我们是 **`mcp.json` > `mcp_servers` DB 行**；请求级绑定（`resolve_mcp_servers`）保持既有优先级排在声明之前 |
| 凭据 | `env` 支持 `secret:NAME` 引用 + `config.toml [credentials]`（比参考实现更强） | 明文值只存在于用户自己的文件里；不进备份 / 快照 / DB（因为不投影进 DB，见下） |

**为什么声明不投影进 `mcp_servers` 表**（`21` D14 ②=C）：投影会让声明文件与导入器 / UI **争同一行**（`McpConfigService::add_server` 是按名 upsert），并引入「文件删了、DB 行还在」的 GC 问题。代价是可见性收窄——文件声明的 server 不进 `connector/*` 目录、不可被 preset `mcp_server_ids` 引用、没有持久化的 `last_test_status` / `tools`。这条边界与实现同批登记，升级为投影时需逐条重写。

**server key 的校验规则（有推导）**：引擎的 provider 可见工具名是 `mcp__` + `sanitize(server__tool)` 截断 + `__` + 16 位摘要，总长上限 64（`nomi-mcp/src/tool_proxy.rs:27-36,447-478`），故 slug 预算 = `64 - 4 - 2 - 16 = 42`；要求 server 自带的分隔符完整存活 → **key 长度 ≤ 40**（保守上界：前缀匹配本身容忍略多一点，取 40 是为了让规则不依赖「分隔符恰好被截断」这种巧合；该上界由跨 crate 测试 `declaration_keys_stay_addressable_by_a_whole_server_pattern` 对 1..=40 全部长度逐个钉住）。又因 `sanitize_display_slug` 会把非 `[A-Za-z0-9_-]` 字节替换为 `_` 并 `trim_matches('_')`，key 必须匹配 `^[A-Za-z0-9](?:[A-Za-z0-9_-]*[A-Za-z0-9])?$`，否则用户写的 glob 与真实工具名对不上。两条都在解析期硬拒绝并给出可操作错误。

#### 7.9.1 参考实现可选字段的落点，与唯一的有界差异

| 字段 | 落点 | 行为 |
|---|---|---|
| `cwd` | `nomifun-app/src/services.rs` 读盘时**相对声明文件所在目录**解析成绝对路径 → `McpServerConfig.cwd` → `nomi-mcp` 的 `SpawnSpec.cwd` → `ChildProcessBuilder::current_dir` | 只换**子进程**目录，宿主目录不变。相对路径必须相对**声明文件**：同一个 `mcp.json` 会被从任意启动目录加载，相对宿主进程 CWD 会让一份文件在不同启动下含义不同。`cwd` 进 `SpawnSpec`，故 respawn 落在同一目录 |
| `bearerTokenEnvVar` | `factory/nomi.rs` 的 `apply_bearer_token`，**在宿主侧解析**，引擎根本看不到这个字段 | 名字（不是值）→ `secret_ref` 查表（`[credentials]` 优先、进程环境兜底）→ `Authorization: Bearer <value>`；显式 `headers.Authorization` 优先（声明是逐字写的）；查不到就**不发这个 header** 并告警，绝不下发 `Bearer <name>` |
| `startupTimeoutMs` | `McpServerConfig.startup_timeout_secs` → `nomi-mcp/src/manager.rs` 的 `startup_timeout_for` | 覆盖连接握手预算（spawn + `initialize` + `tools/list`）的 30s 默认值；越界在**连接之前**拒绝该 server 并告警，不夹取 |
| `toolTimeoutMs` | `McpServerConfig.request_timeout_secs`（既有字段，未变） | 单次 JSON-RPC 调用的整段墙钟预算 |
| `enabledTools` / `disabledTools` | `McpServerConfig.{enabled_tools,disabled_tools}` → `nomi-mcp/src/tool_proxy.rs` 的 `McpToolFilter`，**在 `register_mcp_tools` 构造 proxy 之前**裁剪 | 见 §7.9.2 |

**唯一的有界差异**：`startupTimeoutMs` / `toolTimeoutMs` 参考实现允许 `1..=2147483647` ms，我们按引擎自己的上界（`MCP_MAX_REQUEST_TIMEOUT_SECS` / `MCP_MAX_STARTUP_TIMEOUT_SECS = 600`）卡在 `≤ 600000` ms，**越界即拒绝该条目**。为什么不夹取：一个挂死的 MCP 调用会把整个 agent turn 拖住一小时，这是我们主动不要的能力；而声明文件是用户唯一的意图表达，静默改小会让「这个 server 从来不返回」无法归因。代价是**「把参考实现的 `mcp.json` 原样拷过来」在这里可能失败**——但失败信息会点名具体字段与合法区间，不是静默降级。

#### 7.9.2 `enabledTools` / `disabledTools`：裁剪点选在注册源头

**裁剪点不在注册表策略层，而在注册源头**——这是这批改动成本的关键。`register_mcp_tools`（`nomi-mcp/src/tool_proxy.rs`）本来就已经拿到 `server_configs`，在构造 proxy **之前** `retain` 即可，于是三个回归面全部**由构造自动正确**，一行都不用改：

| 面 | 为什么自动正确 |
|---|---|
| 工具名 / alias | canonical 名的摘要取自 `server__tool` **身份**，存活工具一个字都不变，不存在重命名或哈希漂移 |
| `deferred` 与 `ToolSearch` | deferred catalog 由 `register_batch` 喂入；被裁掉的工具从未进入，搜索自然找不到——这正是白名单语义 |
| `context_usage` | `classify_tool` 按 `mcp__` 前缀对**实际发出的** `ToolDef` 分桶，工具少则 `mcp_and_dynamic_tools` 自动变小 |

反过来，先注册再裁是行不通的：注册表的 `retain_named` / `deny_named` 是**注册表全局**策略，A server 声明白名单会连带砍掉 B server 的工具，per-server 语义在那里根本表达不出来。

**匹配规则（超集，兼容两种读法）**：参考实现只写「工具白名单 / 黑名单：`string[]`」，没写条目是裸工具名还是 `mcp__<server>__<tool>` 全名，两种读法都成立，所以两种都收：

1. 条目以 `mcp__` 开头 → 当 glob，先匹配**原始来源名** `mcp__<server>__<tool>`（逐字对应参考实现的命名），再匹配我们的 canonical 名 `mcp__<server>__<tool>__<hash>`（从 Nomi 工具列表拷来的写法因此也能命中）；
2. 其余条目 → 当 glob 匹配该 server 的**原始工具名**（`read_file` 这种 server 局部名）；
3. `*` 在两条分支里都表示「这个 server 的全部工具」。

第 3 条**故意与 `[tools]` 的策略不同**：`nomi-tools` 里 `mcp__` 之外的裸 `*` 是刻意「不匹配任何东西」的（`registry.rs` 的 `tool_name_matches`），那条规则保护的是**全局**名字空间；而这里的匹配已被限定在一个 server 内，`*` 只可能读作「本 server 全部」。`enabledTools` 先应用、`disabledTools` 后应用，同一模式同时出现在两个列表里 = 排除。glob 解析失败一律**不匹配**（与注册表同样的 fail-closed）。

**两条诊断**（否则「被裁到空」与「这个 server 本来就没这个工具」在界面上完全一样）：

- 白名单条目一个都没命中 → warn 点名 server 与该条目（它是**在裁工具而不是选工具**，几乎总是笔误）；
- 白名单把该 server 的全部工具裁光 → warn 点名 server 与它声称提供的工具数；server 仍然连接（`tools/list` 之前不可能知道工具名），只是不贡献任何工具。黑名单未命中只记 debug——共享的减项清单覆盖多个 server 是正常用法。

**已知边界（有意）**：`disabledTools` 只管**工具**。MCP resources 走 `nomi-skills` 变成 skill，不是工具，因此「禁掉危险工具」不会阻止该 server 的 resource 变成 skill 注入提示词。参考实现是连 resources / prompts 一起过滤的（<https://github.com/HKUDS/nanobot/pull/4524>），我们暂不对齐。

**动态注册没有过滤面（有意）**：引擎运行时的 `AddMcpServer` 只带传输与凭据，没有 `enabledTools` / `disabledTools` 字段，所以 `register_single_server_tools` 不接过滤参数——这是「没有来源」，不是「忽略了来源」。声明（有这两个字段）走 bootstrap 的 `register_mcp_tools`。

#### 7.9.3 `headers` 与 `env` 的 `secret:NAME` 语义已收敛

第四批拒绝了 `bearerTokenEnvVar` 并把 header 凭据指向 `secret:NAME`，于是**声明路径**必须真的解析 header 里的引用；但 DB 行（`row_to_mcp_server_config`）与会话快照路径当时只解析 `env`，把 `headers` 原样下发。这个不一致是**静默的**：写到 header 里的引用会被当字面量发出去，远端 401，本地没有任何线索。

本批把三条路径统一走 `resolve_header_secrets`（`factory/nomi.rs`）：按整值引用解析（`secret_ref::parse_secret_ref` 是**整值**精确匹配），解析不到的条目**丢弃并点名**，普通值原样透传。另外补一条针对最常见误写的告警：值里**含** `secret:` 但**不是**整值引用（典型 `Authorization: Bearer secret:TOKEN`）会原样发出，故 warn 出 server 名与 header 名——**只记 header 名，不记值**。这个形状的正确写法是 `bearerTokenEnvVar`，或把 `Bearer ` 前缀放进凭据值本身。

#### 7.9.4 `deferred`：参考实现的第十个字段，我们**拒绝**（登记为未做）

参考实现在本文成文之后给 `mcp.json` 加了第十个可选字段 `deferred`——它的「按需加载工具」：该 server 的工具不进模型顶层列表，模型改用内置 `select_tools` 按需加载完整定义，且要同时满足实验开关（`KIMI_CODE_EXPERIMENTAL_TOOL_SELECT` / `[experimental] tool-select`）与该模型声明 `dynamically_loaded_tools` 能力。

**我们不在声明里接受它**：`RawServer` 是 `deny_unknown_fields` 的，`deferred` 因此按 §7.9.1 的未知字段规则**拒绝该条目**并点名。除两个超时的上界（§7.9.1）之外，它是照抄参考实现的 `mcp.json` 时**唯一**会被整条目拒绝的字段，所以我们把这条登记写得比一个「未知字段」更清楚，避免用户以为是自己写错了。

**为什么是「未做」而不是「顺手接上」**：引擎侧有同一套机制的另一半（`McpServerConfig.deferred` + `ToolSearch` 目录），但**声明路径固定写 `Some(false)`**，与 `mcp_servers` 行同口径——`merge_host_declared_mcp_servers`（`factory/nomi.rs`）的三个传输分支都是。把它接进来等于**新增一个可选能力**，而不是补一个被漏掉的字段；且必须一并决定三件事，缺一件都会变成静默降级：

1. `deferred` 要不要进读面（`AppServerConfigMcpServerView`，现在只报 `name` / `transport` / `enabled`，见 `05` §4.10）。进 = 读面加字段 = 指纹 bump 加两仓同步；不进则用户在界面上看不到自己声明的状态。
2. 与 `[tools] disabled` 的相互作用。`ToolSearch` 被点名关掉时 deferred 工具不可达（`nomifun-api-types` 的 `NomiToolPolicy::disabled` 已就此告警）。今天宿主侧唯一的 deferred 来源是**自家接线**——gateway MCP 是 `factory/nomi.rs` 里唯一写 `Some(true)` 的一处，其余（DB 行、会话快照、声明）全是 `Some(false)`；声明一旦接受 `deferred`，就等于把这个状态第一次交给用户可写的一份文件，那条告警的适用面随之改变。
3. 参考实现「前提不满足时该字段被忽略」的**静默降级**我们不采纳（同 §7.9.1 的理由：声明文件是用户唯一的意图表达）。要支持就得给出拒绝或告警的判据，而不是忽略。

在上述三点有结论之前，用户侧的写法是：**不要写 `deferred`**；要按 server 收敛工具面用 `enabledTools` / `disabledTools`（§7.9.2），要全局收敛用宿主 `[tools] disabled`。

### 7.10 环境变量覆盖：`AGENT_STORE_TOOLS`

`apps/agent-store` 解析 `[tools]` 时先看环境变量 `AGENT_STORE_TOOLS`：存在且可解析就用它，文件里的 `[tools]` **整张被忽略**（`resolve_host_tool_policy`，`crates/backend/nomifun-app/src/services.rs`）。

- **值就是策略文档**（JSON，形状与 `[tools]` 表一致，即 `NomiToolPolicy` 的 serde）：`{"web":true,"domains":{"cron":false}}`；`{}` 是显式的「全部默认开」（不受限）。
- **整份替换，不是合并**。合并只能收窄（各层策略都是减项，见 §7.2），那就永远无法把模板关掉的域重新打开——而这正是「调用方自己 spawn 一个宿主」需要的；并且「部分合并」会让调用方没点名的键静默回落，比「这个值就是策略」更难预期。替换还让被 spawn 的宿主变确定：SDK 后端拿到它要的 toolset，而不是开发者本机 `~/.agent-store/config.toml` 恰好写着什么。
- **与文件同一个 opt-in 闸门**（`--adopt-store-tool-policy`，只有 `apps/agent-store` 打开）：桌面 / Web 宿主两样都不采纳，所以也无法通过环境变量收窄它们。
- **不可解析 → warn 后回落文件**（与「文件不可用回落宽松默认」同一条 fail-open 口径）；**空值 = 未设置**，不是「全禁」，要全禁请写具体策略。
- 环境变量优先于文件，因此只要它还在，UI 经 `config/set` 写的 `tools.*` 在下次启动**也不会生效**（启动日志会点名来源，便于判断）。
- 启动日志：`host tool policy taken from AGENT_STORE_TOOLS; the config file's [tools] table is ignored`，紧跟着仍是 `agent-store [tools] policy adopted for this host enabled=… disabled=… unrestricted=…`。
- **SDK 侧无需新 API**：`launchClient` 的 `SpawnOptions.env` 已经合并进子进程环境，直接传即可（见 `web/packages/sdk/README.md`）。

---

## 8. Team 与 `nomi_delegate`（强制保留的特例）

依据 `16` §7 决策 3：Team Run 由 Leader 模型调用 `nomi_delegate(strategy=planned)` 触发。因此在 Store 会话里 **`nomi_delegate` 不是可选工具，而是 Team 的必需入口**。

但「保留 `nomi_delegate`」必须落到正确的实现上——仓库里有两个同名实现，能力完全不同：

| 实现 | 位置 | `strategy` | 持久化 | 适用 |
|---|---|---|---|---|
| Platform Gateway 版 | `nomifun-gateway/src/caps_agent_execution.rs`（`DelegateParams` `:147`、`delegate` `:751`） | `planned` / `parallel` | ✅ 走 `AgentExecutionEngine`，落库 Run/Event/Attempt | **Team 必须用这个形态** |
| embedded 版 | `nomi-agent/src/local_delegate_tool.rs` | **仅 `parallel`** | ❌ 现场 `AgentExecutionId::new()`，`execute_fanout` 同步返回，无任何写入 | 仅适合 CLI/嵌入式宿主；**不得用于 Store / Team** |

> 注意：这不等于「Store 要放开 Platform Gateway」。`nomi_delegate(planned)` 目前**恰好**只被 gateway caps 模块实现，但 App Server 本身不依赖 gateway——正确做法是在进程内提供一个绑定 `AgentExecutionEngine` 的等价入口（`AppServerRouterState.runtime: Option<AgentRuntimeAdapter>`（`nomifun-app-server/src/lib.rs:924-928`）已持有该 engine），而不是把 gateway 的 135 项能力整体放开。

### 8.1 Store 会话当前的实际装配（错误组合）

| 装配项 | 当前值 | 应有值 |
|---|---|---|
| `gateway_mcp_config` | `None`（`platform_gateway_entitled` 排除，`factory/nomi.rs:310`） | `None`（Store 不用 gateway） |
| `install_embedded_agent_execution` | `true`（`!has_platform_gateway && is_instance_owner`，`:1500`） | **`false`**（由宿主组装决定，不可配置） |
| planned 入口 | **不存在** | 必须存在（in-process，绑 engine） |
| Connector(MCP) | **被清空**（ceiling + `!is_app_server_chat` 守卫） | 必须保留，且限定为 Definition 绑定的 id |
| 绑定 Skill | **不存在**（空快照） | 必须保留 |

即：模型拿到的是一个「只支持 parallel、且不落库」的壳，同时 Connector 与 Skill 都没了。

---

## 9. 实施步骤

按依赖排序；Step 1–3 是纯管道（缺省值 = 今日行为），Step 4–6 才是行为变更。

| # | 事项 | 位置 | 状态／行为变化 |
|---|---|---|---|
| 1 | `NomiToolPolicy` DTO：`enabled` / `disabled` 双列表 + `web/computer/browser/plan/lsp` + `domains`，`Default` = 全开；`overlay()` 表达「更严者胜」；`syntax_warnings()` 出三类语法告警 | `nomifun-api-types/src/tool_policy.rs`（`app-server` 与 `ai-agent` 均已依赖，不新开 nomi-* 依赖） | ✅ 已落地（无行为变化） |
| 2 | `AgentStoreConfig` 增 `[tools]`（文件形状直接复用 `NomiToolPolicy`，两者不可能漂移）→ `tool_policy()`；读面 `config_view`、写面 `AgentStoreConfigPatch.tools` + `with_tool_lists`/`with_tool_switch`/`with_tool_domain` 三个最小改写助手 | `nomifun-app-server/src/agent_store.rs`、`src/lib.rs`（`config_view` / `execute_config_set`） | ✅ 已落地（未配置即不变） |
| 3 | 桥进工厂：`AgentFactoryDeps.tool_policy`；**启动读一次** → `AppServices.tool_policy`（`resolve_host_tool_policy`，纯函数可测）；宿主位 `--adopt-store-tool-policy`，仅 `apps/agent-store` 置位 | `factory/mod.rs`、`nomifun-app/src/services.rs`、`cli.rs` / `config.rs` / `bootstrap/environment.rs` | ✅ 已落地（无行为变化） |
| 4 | 消费 policy：computer/browser 置 `Some(false)`；cron/meeting/listen 守卫与 requirement/knowledge/learning sink 加 domains 判定；companion/knowledge/goal 清 overrides（含 DB 恢复的 goal）；media 在 manager 跳过；web/plan/lsp 在 **manager** 落 `config.tools.*` | `factory/nomi.rs`（`apply_host_tool_policy`）、`manager/nomi/agent.rs` | ✅ 已落地（有行为变化） |
| 5 | `builtin_denylist` 减项 + `builtin_deny_all` 内部标记；`ToolRegistry` 持久 `disabled` + `deny_named()` + 模式匹配（内置精确、`mcp__` glob）+ 两类 no-match 诊断；bootstrap 应用与告警 | `nomi-config/src/config.rs`、`nomi-tools/src/registry.rs`、`nomi-agent/src/bootstrap.rs` | ✅ 已落地（有行为变化） |
| 6 | Connector／Skill 栅栏：create seam 收 `AppServerChatBindings`，写 `selected_mcp_server_ids`（空 = 硬栅栏）与 `preset_enabled_skills`；ceiling 由「清空 `mcp_server_ids`」改为「缺省补 `Some([])` 栅栏」并放开 Connector 行的读取 | `nomifun-conversation/src/service.rs`、`factory/nomi.rs` | ✅ 已落地（有行为变化） |
| 7 | in-process planned delegate + `team/run`（`16` §7 决策 3） | 新 sink（绑 `AgentExecutionEngine`）+ `nomifun-app-server` 路由 | ✅ **已落地**：模型契约、宿主 sink 接缝、宿主组装开关、engine-backed provider、Store 关闭嵌入版、`team/run` 协议面与 Team 层委派放行（见 §9.2／§9.2.1／§9.2.2） |
| 8 | MCP 声明文件接入（`21` D14）：`~/.agent-store/mcp.json` 的用户级 `mcpServers` → 宿主**启动读一次** → 会话构建时并入 `extra_mcp_servers`（**声明优先于 DB 行**），不投影进 `mcp_servers` 表；`[tools]` 仍在最后求交；宿主位 `--adopt-store-mcp-declarations` 仅 `apps/agent-store` 置位 | `nomifun-api-types/src/mcp_declarations.rs`（新）、`nomifun-app/src/services.rs`（`resolve_host_mcp_declarations`）、`nomifun-ai-agent/src/factory/nomi.rs`、`nomifun-app-server/src/lib.rs`（`config/get` 读面） | ✅ **已落地**（规格见 §7.9，落地记录与验证读数见 §9.3；协议指纹 bump 到 `2026-09-13`） |
| 9 | 声明文件补齐参考实现的**全部**可选字段（`cwd` / `bearerTokenEnvVar` / `startupTimeoutMs` / `enabledTools` / `disabledTools`），并把 `headers` 的 `secret:NAME` 语义在三条装配路径上收敛为一个函数 | `nomifun-api-types/src/mcp_declarations.rs`、`nomi-config/src/config.rs`、`nomi-mcp/src/{tool_proxy,manager,transport/stdio}.rs`、`nomifun-ai-agent/src/factory/nomi.rs`、`nomifun-app/src/services.rs`、`nomifun-common/src/secret_ref.rs` | ✅ **已落地**（规格见 §7.9.1–§7.9.3，落地记录见 §9.4；**无协议变更**，指纹保持 `2026-09-13`） |
| 10 | 读面补 `adopted`：让设置页能区分「文件里声明了」与「本宿主真的在用」（`servers` 描述文件，`adopted` 描述宿主） | `nomifun-api-types/src/app_server.rs`、`nomifun-app-server/src/lib.rs`、`nomifun-app/src/{services,router/routes}.rs`、`web/src/{lib/client.ts,components/dialogs/McpSettingsSection.tsx}` | ✅ **已落地**（2026-09-14，规格与判断见 §9.5；协议指纹 bump 到 `2026-09-14`） |

Step 7 单独成批：它引入新架构件（`nomi_types::Tool` 实现 + App Server runtime wiring），且必须保持 `Planner`/`Router`/`Scheduler`/`AttemptRunner` 私有（`check-agent-vocabulary.mjs:337-340`）。

### 9.1 落地记录（2026-09-11）

Step 1–6 已落地。实现过程中发现并处理的偏差，均已在代码注释与本文件相应章节登记：

1. **白名单求交的空集陷阱（新机制 `builtin_deny_all`）**。manager 现在把引擎配置 × 会话白名单 × 宿主 `enabled` **求交**（原实现是会话层直接覆盖引擎层，破坏了「只收窄」不变量）。但 `builtin_allowlist` 的**空值语义是「不限制」**，所以「各层都有约束、彼此不相容」求交为空时，直接传空会把**最严变成最松**。为此 `ToolsConfig` 增一个 `#[serde(skip)]` 的内部标记 `builtin_deny_all`（永不从文件读取），bootstrap 据此显式 `ToolRegistry::clear()`。已由 `incompatible_allowlist_layers_deny_everything` 端到端锁定。
2. **`mcp_server_ids` 的 `None` ≠ 空**。原 ceiling 把它置 `None`，而 `load_user_mcp_servers` 的 `None` 语义是「绑定宿主**全部** enabled MCP」——B6 保留 Connector 后这会变成泄露。现改为：缺省时补成 `Some(vec![])`（硬栅栏），**不覆盖**已绑定的 id。`session_mcp_servers`（桌面请求概念）仍由 ceiling 清空并保持 owner-only，未放开。
3. **诊断作用域**。白名单 no-match = 告警（它在裁工具而不是选工具）；减项 no-match = debug（宿主级清单会合法地覆盖本会话未接线的域）。二者在 bootstrap 于注册后计算，因此只覆盖 bootstrap 注册的工具；由 `[tools].domains` 管辖的域 sink 在 build 之后注册，不在该判定范围内。为使减项的诊断不被自身生效动作掩盖，未匹配项在**应用减项的那一刻**快照（`disabled_unmatched`）。
4. **生效时机**。`[tools]` 与 `[memory]` 同款：**启动读一次**，`config/set` 的写入下次启动生效；`config/get` 立即回读磁盘真实值。
5. **create-time 输入不持久**。`exclude_auto_inject_skills` 与 `preset_enabled_skills` 在 create 时被消费进冻结的 `extra.skills`，**不会留在** stored extra 里；验证方式是断言 `extra.skills` 的最终内容（已由 `app_server_chat_binds_exactly_the_definition_connectors_and_skills` 覆盖，同时锁定 `app_server_chat` 标记必须持久——`list`/`get` 全靠它过滤）。
6. **非 owner 会话会整体清空 extra**（`service.rs` 的 model-only 分支 `req.extra = json!({})`）。因此该 seam 的绑定只在 installation owner 身份下生效；测试必须用服务自身的 owner id。

**验证读数**：`nomi-config` **210 passed / 0 failed**（1 ignored）；`nomi-agent` **747 / 0**；`nomifun-ai-agent --lib` **978 / 29 failed**（29 例全部是 Windows 上缺 `sh` 的 `cli_process`/`acp` 环境性失败，与本次改动无关）；`nomifun-app-server --lib` **115 / 0** 与 `config_set*` **6 / 0**；`nomifun-conversation --lib` **587 passed / 5 failed**（5 例为**既有基线红**，已用 `git stash` 在未改动基线上复现同样 5 例，分别落在 `effective_model` / `runtime_options` / `runtime_state` / `stream_relay`）；`web` **399 passed / 1 skipped**；`cargo check --workspace --tests` 通过（仅既有 warning）；`check:docs-sync` **9 页 0 drift**；`check:agent-vocabulary` 仍为**同样的 8 处既有基线红**（`orchestration` 措辞，均不在本次改动文件内，未新增）。

### 9.2 Step 7 第一批：模型契约 + 宿主 sink 接缝（2026-09-11）

已落地的部分（**不改变任何现有会话的工具面**，因为没有任何宿主安装 provider）：

| 事项 | 位置 |
|---|---|
| `nomi_delegate` 的**计划式**模型契约：`{"strategy":"planned","goal":"…"}`，`deny_unknown_fields`；`max_parallel` / `plan_gate` / `adaptation_policy` / `work_dir` / `members` 一律**拒绝并点名**（`16` §7 决策 3「不接受模型输入」） | `crates/agent/nomi-agent/src/host_delegate_tool.rs`（`HostDelegateTool` + `HostDelegateSink`） |
| 每个会话一个 sink（与 cron / meeting 同形），模型无法寻址其它会话 | 同上 + `NomiAgentManager::register_delegate_sink` |
| 宿主组装开关：`AgentFactoryDeps.embedded_agent_execution` + `should_install_embedded_agent_execution` 第三个输入；CLI `--no-embedded-agent-execution` → `AppConfig.install_embedded_agent_execution`（**默认 true = 全仓保持今日行为**） | `factory/nomi.rs`、`nomifun-app/src/{cli,config,bootstrap/environment,services}.rs` |
| 晚绑定槽 `DelegateSinkProviderSlot`（`OnceLock`，同 `BrowserLaneClientProviderSlot`）：`AppServices` 先建工厂、engine 后建，故由 composition root 安装；槽为空 = 该宿主没有可委派的持久执行面 → **不注册该工具** | `factory/delegate.rs`、`AppServices.delegate_sink_provider_slot` |

**待做（Step 7 剩余）**：①在 `router::state::build_agent_execution_engine` 里实现并安装 engine-backed provider（planned：解析 leader 会话 → model pool → `create_from_conversation` / `create_from_template_for_conversation`，attempt 内则 `delegate_from_attempt` 追加）；②`apps/agent-store` 置 `no_embedded_agent_execution`（必须与①同批，否则会先拿走 Store 唯一的委派入口）；③`team/run` 路由 + Leader Conversation + `execution_template_id` 绑定 + Definition → `AppServerChatBindings`；④`apply_app_server_chat_ceiling` 目前无条件强制 `DelegationPolicy::Disabled`，Team 层需要显式决策如何放行（见 §9.1 第 6 条）。

#### 9.2.1 第二批（2026-09-11）：provider 安装 + Store 关闭嵌入版

**①②已落地**：

| 事项 | 位置 |
|---|---|
| `EngineDelegateSinkProvider`：持 `Arc<AgentExecutionEngine>` + `ConversationService`，`sink_for(owner, conversation)` 给出该会话的 sink；`plan(goal)` = 取 leader 会话 → 拒 `DelegationPolicy::Disabled` → 若该会话已是某执行的 attempt 则 `delegate_from_attempt` 追加，否则按 `execution_template_id` 走 `create_from_template_for_conversation` / `create_from_conversation`；`plan_gate`/`adaptation_policy`/`max_parallel` 全部由宿主给定，**goal 是模型唯一的输入** | `crates/backend/nomifun-app/src/app_server_delegate.rs` |
| 安装点：`build_agent_execution_engine` 在 engine 建好后 `install_engine_delegate_sink_provider(...)`；失败仅记 error（该 host 退回"没有 host-backed 委派"） | `nomifun-app/src/router/state.rs` |
| Store host 关闭嵌入版：`cli.no_embedded_agent_execution = true`（与 `adopt_store_tool_policy` 同处） | `apps/agent-store/src/main.rs` |

**实施中发现并修掉的一处真问题**：provider 是**进程级**安装的（`build_agent_execution_engine` 对所有 host 都跑），因此若只按"槽已安装"就去注册，桌面/Web host 会同时拥有嵌入版与 host-backed 版两个同名 `nomi_delegate` → 第二次注册被 `can_register_route` 判为重复路由而静默拒绝（只留 warning），语义含糊。现已把宿主组装结果提到一个局部变量，注册条件加上 **`!install_embedded_agent_execution`**，把"二选一、绝不同时"变成代码里的显式约束（也正是 `host_delegate_tool.rs` 模块注释所声明的）。

**验证**：`host_composition_switches_the_delegate_deployment`（三态：默认=嵌入版在场 / 关闭后=缺席 / 关闭+provider=同名 host-backed 版回归）、`factory::delegate::tests`（空槽=无工具、二次安装=Conflict、克隆可见性）、`app_server_delegate::tests`（workspace/model pool/use_model/会话 id 校验 5 例）、`cargo check --workspace --tests` **0 error**、`check:agent-vocabulary` 仍是同样的 8 处既有基线红。

**仍未覆盖**：工厂里"槽 → 注册"这段接线的端到端断言。原因是 `AgentRuntimeHandle` 没有公开的工具名查询接口（`AgentRuntimeHandle` 只有生命周期/模式类方法），集成测试无法读注册表；已用 manager 级三态测试 + `should_install_embedded_agent_execution` 单测覆盖其两侧，中间那两行由编译与单测约束。

**③④待做**：`team/run`（公开协议新增，需同步 `05`/`07`/`10` 与 SDK 面、bump 指纹）与 Team 层的 `DelegationPolicy` 放行决策。

#### 9.2.2 第三批（2026-09-11）：`team/run` + Team 层委派放行

**③④已落地**：

| 事项 | 位置 |
|---|---|
| ④**委派闸门从工厂 ceiling 移到受信任的建会话接缝**：`apply_app_server_chat_ceiling` 不再改写 `delegation_policy`——它是会话的一等类型化字段，层级由「哪个接缝创建了它」决定（`create_app_server_nomi_chat` 写 `Disabled`，Team 接缝写 `Automatic`）。伪造 `app_server_chat` 标记不能放大任何东西：该标记**只做减项**，而没有它的会话本来就带着自己的策略 | `factory/nomi.rs` |
| ④**按部署选择委派提示**：同名 `nomi_delegate` 下有三种实现，提示必须描述**真正拥有这个名字的那个**。原先的判据是「有没有 gateway」，而 App Server 会话**永远没有** gateway（`platform_gateway_entitled` 显式排除 `is_app_server_chat`），于是 Team Leader 会拿到已注册但从不被提及的工具。新增 `DelegateDeployment`（Gateway / HostFacade / Embedded / None）与 planned-only 的 `HOST_DELEGATE_STANDARD_HINT`——宿主 facade 版**不能**复用 gateway 文案，后者教模型用 `strategy=parallel` 与 `nomi_execution_get`，而这两者在该部署下不存在 | `factory/nomi.rs` |
| ④**Team 建会话接缝**：`AppServerTeamLeaderBindings` + `create_app_server_team_leader_chat`（与单 Agent 接缝共用私有实现，栅栏不可能漂移）；`Disabled` 的 Leader 与空 template 一律拒绝——「不能委派的 Leader」是自相矛盾 | `nomifun-conversation/src/service.rs` |
| ③**Team Definition → 模板物化**：按 `context.agent_store_team_id` 复用；参与者模型优先取 preset 已解析模型、缺失才回退宿主默认模型（且**先解析 Leader 模型再物化**，让两者同源而非巧合）；`workflow_limits.max_parallel` 仅在正整数时生效；`routing_constraints` 等原文进 `context` | `nomifun-app-server/src/team_run.rs` |
| ③**`team/run` 协议面**：DTO + handler + `POST /api/app-server/team/run` + WS `team/run` + `capabilities.team_runtime`（Team 目录 ∧ 执行 facade）；Leader Conversation → 一轮 turn → 用**新引擎 API** `execution_for_lead_conversation` 反查 `lead` link → 公共 `run_id` | 同上 + `nomifun-app-server/src/lib.rs`、`nomifun-agent-execution/src/engine.rs` |
| ③**模板参与者模型不再被 Leader 会话模型顶替**：`plan_via_engine` 在模板分支传 `lead_model: None`。模板的 `sort_order = 0` 就是 Team Definition 的 lead；传会话模型会「用这个聊天恰好用着的模型」替换 Definition 权威，而且当它不在成员池里时 `promote_lead_model` 会直接拒绝整个运行 | `nomifun-app/src/app_server_delegate.rs` |
| ③**Team 的 Connector 面**：`AppServerTeamDetail.connectors` = 该 Team **快照已安装且启用**的 Connector id。成员 Agent 的 `mcpServers` 按 `02` §5.1 只记录、不映射为授权，把它们当可绑定 id 会凭空发明导入从未建立的权限 | `nomifun-app/src/app_server_importer.rs`、`nomifun-api-types/src/app_server.rs` |

**实施中的两处判断，登记在此**：

1. **复用而非改写模板**。Team Definition 变化时不覆盖已存在的模板：模板是用户可编辑的作者数据，静默覆盖会丢掉人工调过的配置。要换版本由用户删除/重建模板（与桌面模板管理面同一契约）。查找窗口上限 200 行，超出则新建。
2. **`TeamRunReceipt` 不是 `AgentRunReceipt`**。后者带 lead preset 的 `preset_revision` / `content_digest`，而 Team Run 没有 lead preset（权威是模板）。复用那个形状只能靠伪造值上 wire，所以 Team 收据只有 `{run_id, status}`。

**验证**：`nomifun-ai-agent` factory **66 / 0**（含新增的 `exactly_one_delegate_deployment_owns_the_name`、`host_facade_delegation_hint_is_planned_only`、`host_facade_delegation_hint_respects_surface_and_policy`、`app_server_chat_ceiling_leaves_the_delegation_tier_untouched`）；`nomifun-conversation` 两个接缝测试 **2 / 0**（单 Agent 写 `Disabled`、Team 写 `Automatic` + 模板绑定 + 两类拒绝）；`nomifun-app-server` `team_run::tests` **5 / 0**；`cargo check --workspace --tests` **0 error**；`web` **399 passed / 1 skipped**（与基线一致；`45 / 64` → `46 / 65` 的方法计数漂移守卫已同步）；`check:docs-sync` 9 页 0 drift；`check:agent-vocabulary` 仍是同样的 8 处既有基线红。

**仍未覆盖（诚实登记）**：`team/run` 的**端到端**断言（真实 Leader 模型调用委派工具 → 引擎物化 DAG）没有自动化测试。原因是它需要一个可编排的 LLM provider 注入完整 App Server 栈，仓库现有测试基座没有这一层。当前覆盖到的是：请求/收据形状、ceiling 读值、模板取用规则、Team 与单 Agent 两个接缝、模板参与者的引擎侧校验（既有 `nomifun-agent-execution` / `nomifun-db` 测试）。`team/run` 的编排本体（解析 → 栅栏 → 模板 → 会话 → 一轮 → 反查 → 映射）刻意保持为一条直线以便审阅。

#### 9.3 第四批（2026-09-12）：MCP 声明文件（`21` D14）

**Step 8 已落地**：

| 事项 | 位置 |
|---|---|
| 类型与解析：`NomiMcpDeclarations::parse`，**file 级 fail-open + entry 级 fail-closed**；三类传输判定（`command` → stdio；`url` 无 `transport` → http；`transport:"sse"` → sse）；`toolTimeoutMs` 向上取整到秒（`1..=600000`）；五个「参考实现有、本宿主不支持」的字段（`cwd` / `bearerTokenEnvVar` / `startupTimeoutMs` / `enabledTools` / `disabledTools`）与任何未知字段**拒绝该条目并点名**；server key 长度 ≤ 40 且限 `[A-Za-z0-9_-]`（§7.9 的推导） | `nomifun-api-types/src/mcp_declarations.rs`（新） |
| 宿主位与解析：`AppConfig.adopt_store_mcp_declarations` / `--adopt-store-mcp-declarations`；`resolve_host_mcp_declarations`（纯函数，缺失/损坏 → **零声明** + 可报告原因）；声明文件取 `config.toml` 的**同级** `mcp.json`；只有 `apps/agent-store` 置位；启动一次性日志（`target: agent_store_mcp`，含 `declared/enabled/refused`） | `nomifun-app/src/{cli,config,services,bootstrap/environment}.rs`、`apps/agent-store/src/main.rs` |
| 会话注入：`merge_host_declared_mcp_servers` 插在 `resolve_mcp_servers` **之后**、`mcp_servers` 行循环**之前**——该循环是 `entry().or_insert(...)`（先到先得），于是优先级自然成为 **请求级绑定 > `mcp.json` > DB 行**；`env` 与 `headers` 都过 `secret_ref::resolve_env`；`deferred = Some(false)` 与 DB 行一致；**`is_instance_owner` 门控**与 DB 行同款 | `nomifun-ai-agent/src/factory/nomi.rs`、`factory/mod.rs` |
| 读面：`AppServerConfigView.mcp { exists, servers[{name,transport,enabled}], rejected[{name,reason}], error }`，只读、**不在 `config/set` 白名单**（声明文件只能手写）；`config/set` 的写后重读天然带上它 | `nomifun-api-types/src/app_server.rs`、`nomifun-app-server/src/lib.rs` |
| 协议指纹：`2026-09-12` → **`2026-09-13`**（8 处代码/夹具 + 2 处站点文档；方法计数不变，`46 / 65` 守卫未动） | 见 `16` §7 决策 4 |

**实施中的三处判断，登记在此**：

1. **声明也做 owner 门控**。stdio 声明＝让会话执行本地进程，远程声明可携带凭据——两者都属「安装级执行权限」，故与 DB 行一样只在 installation owner 身份下生效（`docs/architecture/data-and-storage.zh.md` §安装级执行权限）。这与「不受 `mcp_server_ids` 围栏约束」不矛盾：那道围栏管的是「快照/preset 授予了什么」，而声明的授予者是宿主操作者本身（对应 `21` D14 的「有意行为」）。
2. **读面必须有 `error`**。整份文件解析失败时回 `exists:true` + 空列表 + `error`，否则「文件写坏了」与「文件是空的」在界面上完全一样。
3. **`headers` 也解析 `secret:NAME`，DB 行不解析**。因为 `bearerTokenEnvVar` 被拒后文档把 header 凭据指向 `secret:NAME`，声明路径必须真的支持它；而 DB 行路径（`row_to_mcp_server_config`）只对 stdio `env` 解析、对 `headers` 不解析。这是**既有实现的不一致，本批未改**——**已于 §9.4 收敛**：三条路径统一走 `resolve_header_secrets`。

**验证读数**：`nomifun-api-types --lib` **680 / 0**（新增 19 例）；`nomifun-app-server --lib` **123 / 0**（新增 3 例）；`nomifun-app --lib` **313 / 1**（唯一失败是既有基线红 `commands::stdio_common::tests::at_most_once_retries_undelivered_connection_failures`，本会话早前已用 `git stash` 在未改动基线上复现同一断言）；`nomifun-ai-agent --lib` **992 / 29 failed**（29 例全为 Windows 缺 `sh` 的环境性失败，分布与上一轮完全一致：`capability::cli_process` ×22、`manager::acp` ×6、`factory::construction_guard` ×1；本次新增 6 例全绿）；`web` **399 passed / 1 skipped**（与基线一致）；`check:docs-sync` **9 页 0 drift**；`check:agent-vocabulary` **同样的 8 处既有基线红**（无新增）。

**真二进制端到端（12/12 通过）**：`cargo build -p agent-store` 后以临时 HOME + 临时 data-dir 启动真实 `agent-store.exe`（`--port 0`），用原生 WS 走 `initialize` → `initialized` → `config/get`：① 合法 `mcp.json`（stdio + http + `enabled:false` + `cwd` 反例）→ `exists:true`、按 key 排序的三条 server（传输与 `enabled` 保真）、`bad` 一条被拒且原因点名 `cwd`，启动日志 `declared=3 enabled=2 refused=1` + 逐条 warn；② 坏 JSON → `exists:true` + 空列表 + `error`，启动日志 `could not be parsed … declaring no MCP servers`；③ 无文件 → `mcp: null`，日志 `declared=0`。三种情况下 `config/get` 的响应里都**不出现**任何凭据值（`[credentials]` 的值与 provider key）。

**会话级端到端（第四批补，2026-09-12）**：`declared_mcp_servers_reach_the_session_tool_surface`（`manager/nomi/agent.rs` tests）——用 wiremock 起一个 Streamable HTTP 的 MCP 端点，走**真实的声明合并**（`merge_host_declared_mcp_servers`，为此把它放宽到 `pub(crate)`）→ `NomiResolvedConfig.extra_mcp_servers` → 真实 `NomiAgentManager` → `tool_names()`，四条断言一次跑通：

1. 声明的 server 的工具**真的出现在** provider 可见工具面（canonical `mcp__declared__…`）；
2. 同会话 `[tools] disabled = ["mcp__declared__*"]` 把它**整组拿掉**（把「`[tools]` 是最后一道」从文档变成断言）；
3. `enabled: false` 的声明**不合并**（不连、不注册）；
4. 同一份声明在**非 owner 会话**里一个工具都不出现——这条同时把 owner 门控钉在会话级，并证明第 1 条不是空洞通过（合并被跳过时该前缀恰不存在）。

**仍未覆盖（诚实登记）**：只剩**跨进程 App Server + 真实模型**那一层——「SDK 驱动 `agent/run`，由真实 provider 流式返回的工具列表里出现声明的 server」。它需要一个可编排的 LLM provider 注入完整 App Server 栈，与 §9.2.2 的 `team/run` 缺口同源。其余各层均已有断言：解析与全部校验规则（19 例）、宿主管线与双 flag 门控（2 例）、factory 注入/优先级/owner 门控/空声明（4 例）、**跨 crate 命名契约**（`declaration_keys_stay_addressable_by_a_whole_server_pattern` 对 1..=40 每个 key 长度逐个验证 `mcp__<key>__*` 命中引擎真实 canonical 工具名）、**会话级工具面（4 条断言）**、读面（3 例），以及上面 ①②③ 的真实二进制闭环。

**有意未做（本批范围外，均已登记）**：

1. **项目级 `<workspace>/.agent-store/mcp.json`**——路径约定已写进 §7.9，实现**不读**（`21` D14 ③=C：Store 是常驻服务端、无交互式信任面）。
2. **投影进 `mcp_servers` 表**（`connector/*` 可见、可被 preset 引用、持久化 `last_test_status`/`tools`）——`21` D14 ②=C 定为后续可选开关，未排期；升级时需重写本节的可见性边界。
3. **引擎侧字段**：`cwd`、server 级 `enabledTools`/`disabledTools`、`startupTimeoutMs`（以及与 `toolTimeoutMs` 分开的两段超时）——需要改 `nomi-config::McpServerConfig` + `nomi-mcp`，回归面涉及 alias/`deferred`/`context_usage`；本批以「逐条目拒绝并点名」替代静默忽略。
4. **热更新墓碑态**（参考实现的 `removed`）——我们天然是「新会话才生效」，但删除后已开会话没有可见标记。
5. **WebUI 设置页的 MCP 分区渲染**——读面（`config/get.mcp`）已就绪，UI 渲染未做。

#### 9.4 第五批（2026-09-13）：五个可选字段全量支持 + `headers` 收敛

**已落地**：

| 事项 | 位置 |
|---|---|
| 解析层：`cwd` / `bearerTokenEnvVar` / `startupTimeoutMs` / `enabledTools` / `disabledTools` 全部接受并做类型化校验；`UNSUPPORTED_FIELDS` 退役，未知字段的报错改为附带**可接受字段清单**；两个超时共用 `whole_seconds`（向上取整到秒 + 区间硬校验，不夹取）；过滤条目 trim / 去重 / 拒绝空串，空数组读作「不过滤」；新增 `resolve_cwd_relative_to` | `nomifun-api-types/src/mcp_declarations.rs` |
| 引擎契约：`McpServerConfig` 增 `startup_timeout_secs` / `cwd` / `enabled_tools` / `disabled_tools`（16 处构造点全部**显式**补 `None`，不用 `..Default::default()`：`TransportType::default()` 是 `Stdio`，把它藏进默认值等于给「忘了写 transport」留后门） | `nomi-config/src/config.rs` 及各构造点 |
| 过滤与命名：`McpToolFilter` + `filter_entry_matches` + `glob_matches` + `McpFilterReport`；`register_mcp_tools` 在构造 proxy **之前**裁剪并出两类告警；`register_single_server_tools` 明确「没有过滤来源」（§7.9.2） | `nomi-mcp/src/tool_proxy.rs` |
| 连接：`startup_timeout_for` 取代固定 30s（越界在连接**之前**拒绝该 server 并告警）；`SpawnSpec.cwd` + `spawn_with_cleanup_registry` 透传 + `ChildProcessBuilder::current_dir`（respawn 复用 `SpawnSpec`，天然同目录） | `nomi-mcp/src/manager.rs`、`transport/stdio.rs` |
| 宿主映射与凭据：三种传输映射新字段；`apply_bearer_token` 把 `bearerTokenEnvVar` 落成 `Authorization: Bearer …`；**三条路径**（声明 / DB 行 / 会话快照）统一走 `resolve_header_secrets` | `nomifun-ai-agent/src/factory/nomi.rs` |
| 凭据查表：新增 `secret_ref::lookup` / `lookup_with`，让「按名字取凭据」与 `secret:NAME` 引用共用同一条优先级（`[credentials]` 优先、进程环境兜底），不在调用点重写一遍 | `nomifun-common/src/secret_ref.rs` |
| 宿主读盘：`cwd` 在读盘时相对声明文件目录解析成绝对路径；启动日志（`target: agent_store_mcp`）除 `declared/enabled/refused` 外，逐条 debug 打出 transport / cwd / 两个过滤条目数 / bearer 名 / 两段超时——「声明了什么」是单点事实（一次启动一次），「连上了没有」才是每会话的事 | `nomifun-app/src/services.rs` |
| 协议面：**无变更**。读面仍是 `mcp { exists, servers[{name,transport,enabled}], rejected, error }`，指纹保持 `2026-09-13`（`adopted` 见 §9.5，宿主事实的补位） | —— |

**三处判断，登记在此**：

1. **超集匹配，而不是在两种读法里二选一**。参考实现文档没有定义 `enabledTools` 条目的形态，两种读法都成立，所以两种都收（§7.9.2）。代价是匹配规则比单一读法更容易被误读；收益是「拷过来就能用」在这件事上真的成立。规则与理由写在 `McpToolFilter::selects` 的文档注释里，并由「原始来源名 / canonical 名 / `*` / 跨 server 不误命中」四条断言钉住。
2. **`bearerTokenEnvVar` 在宿主侧解析，引擎不新增字段**。它本质是「`Authorization: Bearer <凭据>`」的语法糖；做成引擎字段只会让引擎多一个与 `headers` 语义重叠的概念，而宿主解析还能天然复用 `secret_ref` 的查表优先级。
3. **§9.3 第 3 条判断作废**：当时登记的「声明路径解析 headers、DB 行不解析」的不一致，已在 §7.9.3 收敛为三条路径同一个函数。这是修正不是回退——`env` 与 `headers` 现在语义一致。

**验证读数**：`nomifun-api-types --lib` **684 / 0**（§9.3 的 680 → 净 +4：替换 2 例、新增 6 例）；`nomi-mcp --lib` **127 / 0**（新增 5 例）；`nomifun-common --lib` **229 / 0**（新增 1 例）；`nomifun-ai-agent --lib` **996 / 29 failed**（§9.3 的 992 → 新增 4 例全绿；29 例仍是那批 Windows 缺 `sh` 的**环境性**失败，分布逐一同前：`capability::cli_process` ×22、`manager::acp` ×6、`factory::construction_guard` ×1）；`nomifun-app --lib` **314 / 1**（新增 1 例宿主级 `cwd` 解析断言；唯一失败仍是既有基线红 `commands::stdio_common::tests::at_most_once_retries_undelivered_connection_failures`）；`nomifun-app-server --lib` **123 / 0**；`cargo check --workspace --tests` **0 error**；`web`（`bun run test`，vitest）**399 passed / 1 skipped**；`check:docs-sync` **9 页 0 drift**；改动文件 `rustfmt --edition 2024 --check` 干净、CRLF 扫描 0（`nomifun-common/src/secret_ref.rs` 的**工作树**原本是 CRLF 而 blob 是 LF——`git status` 看不见这类差异——本次顺手归一成 LF，diff 仍是 41/4）；`check:agent-vocabulary` 仍是**同样的 8 处既有基线红**。

**既有基线红（本批未引入、未修，均已核验归属）**：`bun run check` 在 `ui` typecheck / `check:i18n` / `check:button-layout-contract` / `check:dead-css` 处红，`check:process-runtime-boundary` 在 `apps/agent-store/src/main.rs:439` 红（该行出自 `a135d0a3b`，2026-09-04）。证据是 `git status --porcelain -- ui/ apps/` **为空**：这些门的输入与 HEAD 逐字节相同，而本批只改了 `crates/` 与 `docs/`，未动 `ui/`、未动 `apps/`。

**真二进制端到端（19/19 通过，2026-09-13）**：以临时 HOME（`USERPROFILE`）+ 临时 data-dir 启动真实 `agent-store.exe`，走 `/api/app-server/ws` 的 `initialize` → `initialized` → `config/get`（注意不是 `/ws`——那是另一条 realtime 通道，连上去会静默无响应）。写入一份**把参考实现文档字段全用上**的 `mcp.json`（stdio 带 `cwd` / `startupTimeoutMs` / `toolTimeoutMs` / `enabledTools` / `disabledTools`；http 带 `headers` + `bearerTokenEnvVar`；sse；`enabled:false`；外加两条反例：stdio 上放 `headers`、`toolTimeoutMs: 3600000`）→ `exists:true`、按 key 排序的四条已接受条目（传输与 `enabled` 保真）、两条被拒且原因分别点名 `` `headers` `` 与 `` `toolTimeoutMs` ``、无文件级 `error`，启动日志 `declared=4 enabled=3 refused=2`；响应里**不出现** `[credentials]` 的值、`env` 的值、声明的 `cwd`、过滤条目——即视图仍只报 `name` / `transport` / `enabled`。**一处取不到**：宿主逐条 debug 日志（解析后的绝对 `cwd`、过滤条目数）在这个二进制里读不到——`agent-store` 有自己的小 CLI 且**不转发** `--log-level`——所以那一半由单测兜住（`nomifun-api-types` 的 `cwd_is_resolved_against_the_declaration_file_only_when_relative` 与 `nomifun-app` 的 `declared_cwd_is_resolved_against_the_declaration_file`）。

**仍未覆盖（诚实登记，与 §9.3 同一条缺口）**：跨进程 App Server + 真实模型那一层。本批新增的断言覆盖：解析与全部校验（含边界值 `0` / `600001`）、`cwd` 的「相对文件 / 绝对原样」两分支、过滤的超集匹配与裁空诊断、`bearerTokenEnvVar` 的三种结局（解析成功 / 显式 header 优先 / 查不到则不发）、`headers` 引用的三路径一致 + `Bearer secret:X` 的告警形状，以及**引擎侧真实裁剪**（`register_mcp_tools` 跑完后 `registry.get(canonical)` 为 `None`——工具确实不在注册表里，不只是「未被广告」）。**没有**新增的：需要真实子进程的 `cwd` 端到端（Windows 上 stdio 测试本就缺 `sh`，同 §9.3 的环境性限制）、以及真实模型驱动的一整条链路。

**有意未做（更新后）**：

1. **项目级 `<workspace>/.agent-store/mcp.json`**——不变（`21` D14 ③=C）。
2. **投影进 `mcp_servers` 表**——不变（`21` D14 ②=C）。
3. **`disabledTools` 覆盖 MCP resources / prompts**——见 §7.9.2 的已知边界（它们在我们这里变成 skill，不是工具）。
4. **热更新墓碑态**（参考实现的 `removed`）——不变。
5. ~~**WebUI 设置页的 MCP 分区渲染**~~ → **已完成（2026-09-13）**：设置 nav 三→四，新增 `mcp` 分区（`web/src/components/dialogs/McpSettingsSection.tsx`），只读渲染 `config/get.mcp`（文件级 `error`、逐条 `rejected` 原因、已接受条目与计数），页面上没有任何写控件。**未扩读面**：`rejected` 的原因已经点名出问题的具体字段，比再加 `cwd` / 过滤条目数更有用，所以本次**零协议变更**（指纹未动）。渲染细节与踩到的 i18n 坑见 `16` §5.3 的 R16 追记。

---

#### 9.5 第六批（2026-09-14）：读面补 `adopted`（宿主是否采用）+ 指纹 bump

**为什么加这个字段**：`config/get.mcp` 的 `servers` / `rejected` / `error` 描述的都是**文件**，没有一个字段描述**宿主**。而 `mcp_declaration_view` 是**无条件读盘**的——它不检查宿主有没有开 `--adopt-store-mcp-declarations`。于是「宿主根本不读这份文件」（`nomifun-web` 默认位，或任何没开这个开关的宿主）与「宿主把每一条都注入了会话」，在设置页上**渲染完全一样**。这是第四批只读分区落地时就登记的产品边界（`16` §5.3：面板当时只能写「不代表宿主是否采用它」），本次用一次协议增量收口。

| 事项 | 位置 |
|---|---|
| 契约：`AppServerConfigMcpView` 增 `adopted: Option<bool>`（`#[serde(default, skip_serializing_if = "Option::is_none")]`） | `nomifun-api-types/src/app_server.rs` |
| 宿主位：`AppServerRouterState.adopt_store_mcp_declarations: Option<bool>`——**宿主事实**而非文件事实（`agent_store_config_path` 说从哪读，它说读不读） | `nomifun-app-server/src/lib.rs` |
| 传递：`AppServices` 保留**原始开关**；读面由 `mcp_declaration_view(path, adopted)` 原样带出 | `nomifun-app/src/{services,router/routes}.rs` |
| 前端：`AgentStoreConfigMcp.adopted?: boolean`；设置页新增一行「本宿主是否使用它」，三态文案 | `web/src/lib/client.ts`、`web/src/components/dialogs/McpSettingsSection.tsx` |
| 指纹：`2026-09-13` → **`2026-09-14`**（8 处代码/夹具 + 2 处站点文档；仍无方法增删，`46 / 65` 计数守卫未动） | 见 `16` §7 决策 4 |

**三处判断，登记在此**：

1. **三态而非两态**。`apps/agent-store` 在这个字段存在**之前**就已经在采用声明了。若把「宿主没上报」折叠成 `false`，一个「比 UI 旧、但明明在采用」的宿主会被这一行说成「未使用」——把不知道说成了否定答案。所以 `None` 是独立的一态、界面单独一句文案。与 `mcp: Option<..>`（无文件 ≠ 空文件）、`distill_enabled: null`（未配置 ≠ 关闭）是同一条口径。
2. **放 state，不放进程级全局**。抄 `marketplaces_warming` 那套 `static AtomicBool` 能省掉 state 与 `AppServices` 两处字段，但读面测试就只能建在共享可变全局上，并行测试按线程交错即互相污染。`AppServerRouterState` 已有 `impl Default` 且所有测试点都走 `..Default::default()`，所以新字段**零测试改动**——3 行的代价换掉一类不确定性，值得。
3. **不能用已解析的 `mcp_declarations` 反推开关**。「采用了、但文件里什么都没声明」与「压根没看这份文件」解析结果都是空声明，而只有前者让这份文件在这台宿主上有意义。故 `AppServices` 同时保留原始 `bool` 开关。

**验证读数**：`nomifun-app-server --lib` 的 `config_get_projects_mcp_declarations_and_refusals` 新增三段断言——① `adopted` **缺席**时必须不在 wire 上（`serde_json` 的 `get("adopted").is_none()`，证明 `skip_serializing_if` 生效、三态没有被 `null` 抹平成两态）；② `Some(false)` + 一份完全正常的文件 → `adopted=false` 而 `servers` 照常投影（这正是「文件里有、宿主不用」那一对）；③ `Some(true)` → `adopted=true`。`cargo check -p nomifun-api-types -p nomifun-app-server -p nomifun-app --tests` **0 error**；`cargo test -p nomifun-app-server --lib mcp` **3 / 0**；`web`（`bun run test`）**407 passed / 1 skipped**（新增 1 例三态渲染断言：`false` 说「未使用」、`true` 说「使用中」、缺席说「无法判断」，且三者互不串台）；指纹 10 处全部同步（`web` 侧 `client.ts` 严格相等校验与 `sdk/src/spawn.ts` 的握手校验即在测试里，漏改会红）。

**仍未覆盖**：真二进制端到端里未重跑「`adopted` 是否真的随宿主开关变化」——那需要分别以 `agent-store`（强制置位）与另一个不置位的宿主启动一次并对比 `config/get`。单测覆盖了从 state 到 wire 的整段投影，缺口只在「宿主 CLI 开关 → state」这一段的一行赋值上，由 `routes.rs` 那一行 + `cargo check` 兜住。

---

## 10. 验收关联

| 用例 | 关联点 |
|---|---|
| `TC-TEAM-001` | Leader Conversation 创建 + `execution_template_id` 绑定（§8） |
| `TC-TEAM-002` | Leader 经 `nomi_delegate(strategy=planned)` 触发；实现选择断言（§8） |
| `TC-RT-*` | 单 Agent Run 的工具面符合 §6.1 基线 |
| `TC-CONN-*` | Connector 工具确实进入 Store 会话，且**只有绑定的那个**（§7.3） |
| `TC-SEC-*` | §6.3 关闭项不得出现在 Store 会话工具面 |

工具面的自动化断言建议以 **provider 可见工具名集合**为断言对象（`registry.tool_names()` / `to_tool_defs()`），而不是以配置字段为对象——配置正确但注册顺序变化仍可能改变实际工具面。

现成断言模板：`manager/nomi/agent.rs:6076-6112`（`NomiAgentManager::new(...)` → `agent.engine.lock().await.tool_names()`）。建议三组断言：① 未配置 `[tools]` → 工具面与今日完全一致（防回归，最重要）；② 关 domains → 对应族全部消失，且 Connector 代理与 `ToolSearch` 仍在；③ `deny` 能关掉 `update_plan`（验证减项对无条件注册的工具同样生效）。

---

## 11. 与其他文档的关系

- 架构边界与决策总表：`00-architecture-decision.md`
- 领域模型（工具取舍的判定依据）：`01-domain-model.md`
- Runtime Adapter 与 Team 触发链：`04-flowy-agent-store-runtime-adapter.md`
- 公共契约（不改）：`10-public-contracts.md`
- 决策记录（本文依据）：`16-sdk-webui-site-priority-plan.zh.md` §7 决策 3（Team 触发）、§7 决策 5（工具策略权威来源）
- 同源先例（同一份宿主配置文件的另一段）：`21-open-decisions.zh.md` D3=B（`[approvals]`）
- 测试总索引：`agent-store-v1-test-cases.md`
