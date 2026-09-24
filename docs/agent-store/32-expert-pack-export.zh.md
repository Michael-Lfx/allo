# 32 · 专家 / 专家团导出给外部 runtime（ExpertPack）· 实现方案

> 状态：📋 **待动工**（方向与五个分叉均已拍板，2026-09-24，见 §1.1）。拍板内容：走 **B · 导出式**——
> 外部 runtime **自己跑**我们的专家，**编排由外部 runtime 自行负责**；通道为 **wire 是服务端唯一面**
> （同机工效由站点配方 + 一支 live 脚本承载，**不新增 SDK 公开面**）；团的导出**递归展开且成员缺一即
> 整包失败**；技能**引用不内联**；闸门是**默认开的减项表**（与 `[tools]` 同族，不是 `[connector_proxy]`
> 那种授权表）。本文是动工前的范围与验收口径登记，实施读数与偏差将回写在 §9。
> 触发：用户提问「如果希望将 agent store 的专家、专家团通过 SDK 给外部 agent runtime，应该如何实现」。
> **日期标签说明**：本页的 `2026-09-24` 是**按 `fp-` 谱系取的标签**（紧接 `fp-7` 的 `2026-09-23`），
> **不是日历日期**——`30` §D4 已记过这个习惯（日期戳只是标签，常超前于日历）；wire 身份以 `fp-8` 为准。
> 关联：`24-external-agent-skill-and-mcp-access.zh.md`（外部 Agent 用 Skill / MCP 的姊妹篇）、
> `04-flowy-agent-store-runtime-adapter.md`（allo 是唯一 Runtime；§3.2 persona 组装 / §4 Team 编排 /
> §5 事件与 Artifact）、`05-flowy-agent-store-app-server-protocol.md`（协议正文）、
> `10-public-contracts.md`（§7 错误码 / §8 命名定稿顺序）、`17-plugin-spec.zh.md`、
> `27-conversation-binding-plan.zh.md`（`agent_id` / `team_id`）、`29-send-model-and-effort-plan.zh.md`、
> `30-market-zip-hosting.zh.md`（市场与指纹的既有形状）。
> 用途：回答「外部 runtime 想在自己的循环里跑 Agent Store 的专家/专家团」——界定**给什么、
> 不给什么、它必须自己实现什么**，以及每一步的验收标准。

---

## 1. 结论先行

| 目标 | 现状 | 本方案 | 结果 |
|---|---|---|---|
| 外部 runtime **调**我们的专家 / 专家团 | ✅ 已可用（`agent/run` · `team/run` · `conversation/create` 的 `agent_id` / `team_id`） | 不动 | — |
| 外部 runtime **看**专家 / 团的元数据 | ✅ 已可用（`agent/list` · `agent/get` · `team/list` · `team/get`） | 不动，**且刻意不给正文加字段**（§2） | — |
| 外部 runtime **拿到专家定义**（persona / 模型 / 技能 / 连接器） | ❌ 完全没有：`agent/get` 的 DTO 里根本**没有**承载正文的字段 | `agent/export` · `team/export` → **`ExpertPack`** | 📋 本方案 |
| 外部 runtime **复现团的编排** | ❌ 完全没有 | **不做**——只给「名单 + 意图」，§5 逐条列出它必须自己实现的东西 | 有意（本方案的核心边界） |

**一句话**：本方案把「专家」从**一个绑在我们引擎上的 Preset** 降级成**一份可移植的定义**，
并明确声明这份定义**不含执行语义**——执行语义由 §5 那张清单交给外部 runtime 自己承担。

**这不是 `agent/get` 加一个字段。** expert 的 persona 今天被两处注释明文拒绝上公共面
（`app_server.rs:435`、`frontmatter.rs:114`），那是设计不是欠账；导出必须是**独立的 seam + 独立的闸门**（§6.2）。

### 1.1 拍板记录（2026-09-24，用户逐条选定）

| | 决定 | 与我的默认是否一致 | 落在哪节 |
|---|---|---|---|
| **D8** | **两种都要**：wire 是**服务端唯一面**；同机工效由**站点配方 + 一支 live 脚本**承载 | 方向采纳我列的第三档，**但物化载体被我推翻**（见 §6.5：砍掉 `materializeExpert`） | §2 第 7 条 / §6.5 |
| **D2** | `team/export` **递归展开 + 整包失败** | ✅ 一致 | §6.4 |
| **D3** | 技能**引用**（`name`/`id` → `skill/files`），**不内联** | ✅ 一致 | §2 第 4 条 / §4 |
| **D5** | 闸门**默认开**（`[expert_export]` 是**减项表**） | ❌ **我原先是错的**，详见 §6.2 的自我订正 | §6.2 |
| **D4** | 连接器只给 `{id, name, enabled}`，schema 走 `connector/get` | ✅ 一致 | §4.2 / §10.3 |

D1（两个方法）/ D6（`pack_format` 独立于指纹）/ D7（导出确定性）/ D9（不给参考编排器）**维持原样**。

---

## 2. 边界（非目标）

1. **不导出执行语义**。团队编排、Step 调度、权限与工具策略**都不在数据里**（§5）。本方案不假装能导，
   也不提供「把 allo 的 Planner 搬过去」的路径。
2. **不给 `agent/get` / `agent/list` / `team/get` 加 `instructions` 字段**。那会让「商店浏览面」与
   「导出面」共用一条通道，而门禁只能加在方法级——`agent/get` 是每个商店 UI 都调的方法。
3. **不导出连接器凭据**。沿用 `24` §2/§5.3：pack 里只有 `{id, name, enabled}`，**没有** transport /
   env / headers / token / URL。外部 runtime 要么自己配一份等价的，要么回调 `connector/call`。
4. **不在 pack 里内联技能正文**。技能字节走既有 `skill/files` · `skill/file`（`24` §4 阶段 1 已落地），
   pack 只给**名字**。理由：不制造第二个真相源，也不让一次导出把 289 MiB 的市场重新序列化一遍。
5. **不引入第二套权限模型**，不新增远程 / 多租户能力，不执行 `scripts/`。
6. **不导出 allo 内部 id**（`resolved_agent_id` 等）。它们对外部 runtime 无意义，只会诱导误用（§4.3）。
7. **不新增任何服务端写盘方法**（D8 的直接推论）。物化到目录是**消费者**的事：服务端只有
   `agent/export` · `team/export` 两个**只读**方法。这样就不存在「调用方指定一个路径让宿主去写」这个面
   ——那条路会引入路径穿越、越权写、以及「谁拥有那个目录」三个问题，而它的收益（省一次客户端写盘）
   远小于成本。
8. **也不新增客户端 SDK 物化助手**（§6.5，2026-09-24 拍板）。同一件事在**站点配方 + 一支 live 脚本**
   里以 11 行公开原语完成，不需要一个带 semver 承诺的 API。**服务端与包的公开面在本次改动里
   只增加两个只读方法。**

---

## 3. 设计依据（既有事实，附证据位置）

| 事实 | 位置 |
|---|---|
| **专家 = 一行 Preset**：`instructions`＝persona、`agent_preferences` 钉死内部运行时 agent、model preferences；`included_skills` / `mcp_server_ids` 安装时**写空** | `app_server_installer.rs:157-198`；`NOMI_RUNTIME_AGENT_ID`＝`0190f5fe-7c00-7a00-8000-000000000114`（`:31`） |
| persona 的**原始字节**：SQlite 组件 payload 的 `instructions` 键；磁盘快照 `{work_dir}/agent-store-imports/<snapshot_id>/agents/*.md` | `frontmatter.rs:161-166`（body → `payload["instructions"]`）；`install.rs:42-45`（快照根） |
| 正文**刻意不上公共面** | `frontmatter.rs:114-116`；`app_server.rs:435-436` |
| 专家**声明的技能**是技能**名**，不是 id | `AgentDoc.skills: Vec<String>`（`frontmatter.rs:126`）；`skill/list` 的 `id` 就是技能名（`24` §2） |
| 专家的技能在运行期怎么生效 | `lib.rs:3965-3981`：`PresetOverrides.include_skills = agent.summary.skills` → 解析后的快照 → 会话 `extra.preset_enabled_skills` |
| 团成员的技能怎么生效 | `team_run.rs:258`：`enabled_skills: definition.summary.skills.clone()` → `attempt_runner.rs:618` 的 `preset_enabled_skills` |
| 技能在运行期的**已解析形态已经存在**：`ResolvedSkillSnapshot { skill_id, name, source, version_hash(64 位 sha256), content(正文) }`；声明了却取不到技能是**失败关闭**，不静默少挂 | `nomifun-conversation/src/service.rs:1611-1633`（缺失 ⇒ `PRESET_SKILLS_UNAVAILABLE`）、`:421-449`（校验条数与 sha256 形状） |
| **专家没有「连接器依赖」这个概念**——不是缺失，是格式里根本没有：插件级 `mcpServers` 是**插件启用后的能力**，官方明确**不自动成为每个 Agent 的权限**，导入器必须记 `ignored-by-source-runtime`，**不得**当作 Agent 级 Connector 授权 | `02` §5.1（`02-codebuddy-workbuddy-import-spec.md:95`、`:127`）；`AgentDoc` 因而没有该字段（`frontmatter.rs:118-138`）；`agent_summary.connectors` 硬编码 `vec![]`（`app_server_importer.rs:296`） |
| 团的 `connectors` 只取**该团自己的快照安装并启用**的连接器；成员 Agent 的 `mcpServers` **刻意不取** | `app_server_importer.rs:364-379`（team detail）；理由原文见 `app_server.rs:550-554`：「treating them as bindable ids would **invent authority the import never established**」 |
| 团的策略是**自由形式字符串 + 一个工具策略摘要**，映射不到引擎的结构化类型 | `team_run.rs:249-256` 注释；`AppServerTeamDetail`（`app_server.rs:537-557`） |
| 团策略**原样进 template `context` 给 Planner 看** | `team_run.rs:328-334` |
| 成员模型优先级：preset 自己的 model ＞ 宿主默认；都没有 ⇒ `team_member_model_unbound` | `team_run.rs:212-239` |
| 成员前置校验与码：`agent_not_installed` / `agent_disabled` | `team_run.rs:167-207` |
| 工具权限**由 Runtime Policy 计算，不由 Persona 文本决定** | `04` §3.2 |
| 成员角色约束**必须在 Runtime 代码中校验** | `04` §4.1 |
| Step 调度不变量（依赖、池内下标、`max_parallel` 三者最小、终态不被迟到事件覆盖、retry ⇒ 新 Attempt、replan ⇒ 新 Plan Revision） | `04` §4.4 |
| 事件类型表与 Artifact 字段白名单 | `04` §5 |
| 路由表计数被测试钉死 | `web/packages/client/src/http-transport.test.ts`：**48 映射 / 23 无 HTTP 绑定**（合计 71） |
| 现行指纹 | `APP_SERVER_PROTOCOL_VERSION = "fp-7"`（`web/packages/protocol/src/protocol.ts:60`） |

---

## 4. `ExpertPack` 的形状

### 4.1 DTO

```rust
pub struct AppServerExpertPack {
    pub pack_format: u32,                 // = 1；**独立于 fp-n**（见 §4.4）
    pub kind: AppServerExpertPackKind,    // "agent" | "team"
    pub id: String,                       // agent/list 或 team/list 的 id
    pub version: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<AppServerLocalizedText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub persona: AppServerExpertPersona,
    pub model: AppServerExpertModel,
    #[serde(default)] pub skills: Vec<AppServerExpertSkillRef>,
    #[serde(default)] pub connectors: Vec<AppServerExpertConnectorRef>,
    pub tool_policy: AppServerExpertToolPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<AppServerExpertTeamPack>,   // kind = "team" 时必填
    pub provenance: AppServerExpertProvenance,
    pub runtime_binding: AppServerExpertRuntimeBinding,
}

/// 本次新增的**唯一**正文载体。刻意只在这一条新面上出现（§2 第 2 条）。
pub struct AppServerExpertPersona {
    pub instructions: String,              // = payload["instructions"]（Agent Markdown 正文）
    #[serde(default, skip_serializing_if = "Option::is_none")] pub memory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub background: Option<String>,
}

pub struct AppServerExpertModel {
    #[serde(default, skip_serializing_if = "Option::is_none")] pub declared: Option<String>,  // frontmatter `model` 原文
    #[serde(default, skip_serializing_if = "Option::is_none")] pub resolved: Option<AppServerExpertModelRef>, // provider_id + model
    #[serde(default, skip_serializing_if = "Option::is_none")] pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub max_turns: Option<u32>,
}

pub struct AppServerExpertSkillRef { pub name: String, pub id: String }

/// **只有身份与开关**：schema 走既有 `connector/get`，凭据永不出宿主（`24` §5.3）。
pub struct AppServerExpertConnectorRef { pub id: String, pub name: String, pub enabled: bool }

pub struct AppServerExpertToolPolicy {
    #[serde(default)] pub tools: Vec<String>,
    #[serde(default)] pub disallowed_tools: Vec<String>,
}

pub struct AppServerExpertTeamPack {
    pub lead_agent_id: String,
    pub member_agent_ids: Vec<String>,
    pub planner_policy: String,
    #[serde(default)] pub routing_constraints: Vec<String>,
    pub workflow_limits: serde_json::Value,
    pub team_runtime_capabilities: Vec<String>,
    /// 递归展开的成员定义，**leader 在首位**。见 §4.2 第 6 行。
    pub members: Vec<AppServerExpertPack>,
}

pub struct AppServerExpertProvenance {
    pub source: String,                    // 与 agent/get 的 source 同源
    pub snapshot_id: String,
    pub content_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub preset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub preset_revision: Option<i64>,
}

/// 诚实声明这份定义**绑过**哪个引擎。`portable = false` 是常态，不是错误。
pub struct AppServerExpertRuntimeBinding { pub runtime: String, pub portable: bool }
```

### 4.2 逐字段来源

| 字段 | 来源 | 备注 |
|---|---|---|
| `id` / `version` / `name` / `source` | 组件 payload | 与 `agent/get` 逐字一致 |
| `display_name` / `description` | payload `display_name`（localized）/ `description` | `display_description` 是市场描述，**不放**（§4.3） |
| `persona.instructions` | payload `instructions` | **本方案唯一的新正文面** |
| `persona.memory` / `background` | payload 同名字段 | 已在 `agent/get` 上，此处只为「自包含」 |
| `model.declared` | payload `model`（frontmatter 原文） | 字符串，可能不是本宿主的 provider |
| `model.resolved` | `PresetService.resolve(...)` 的 `resolved_model` | 与 `team_run.rs:208-228` 同源 |
| `model.effort` / `max_turns` | payload 同名字段 | 声明，非强制 |
| `skills[]` | payload `skills`（**名字**） | `id` 与 `name` 同值（`skill/list` 的 id 即技能名）；字节走 `skill/files` |
| `connectors[]` | **团的**快照安装状态（已装且 enabled）；**专家恒为空数组** | 与 `team/get` 同源（`app_server_importer.rs:364-379`）。专家恒空是**正确的、不是缺陷**：该格式没有 Agent 级连接器声明（§3），填它等于**发明授权** |
| `tool_policy` | payload `tools` / `disallowed_tools` | **声明**。`permission_mode` 不放（来源运行时自己就忽略它） |
| `team.*` | `team/get` 的 `AppServerTeamDetail` 同源 | 逐字一致，避免第三份真相 |
| `team.members[]` | 逐个 `agent` 组件递归成 pack | 成员未安装 ⇒ **整包失败**（§6.3） |
| `provenance` | 组件行 + 快照行 | 用于归属标注与漂移检测（外部 runtime 可拿 `content_digest` 做缓存键） |
| `runtime_binding` | 常量 | `{ runtime: "nomi", portable: false }` |

### 4.3 刻意不放的字段

| 不放 | 理由 |
|---|---|
| `avatar_url` | 它指向**本宿主**的公开 assets 路由（`/api/app-server/imports/…`），对远端的用例是死链；远端需要时应由宿主的站点/市场另给 CDN 地址。 |
| `display_description` / `quick_prompts` / `tags` / `category_id` / `expert_type` | 商店**呈现层**元数据，与「外部 runtime 跑这个专家」无关；要展示就继续走 `agent/get`。 |
| `resolved_agent_id` / `preset_id` 之外的内部 id | 外部 runtime 没有这些对象。`preset_id` 保留（已在 `agent/get` 上，便于跨宿主对账）；`resolved_agent_id` **不放**，由 `runtime_binding` 抽象表达。 |
| `connectors[].tools` / `input_schema` | 取它们要走 `connector/get`（可能触发探针），导出面不该产生网络依赖；外部 runtime 需要时自己调。 |
| `exported_at` 时间戳 | 见 §4.4。 |
| `permission_mode` | 来源运行时自己忽略（`compat.rs:29`），导出它等于转述一个不影响行为的字段。 |

### 4.4 两个刻意的形状决定

1. **`pack_format` 独立于 `fp-n`。** `fp-n` 管的是**我们的** wire 兼容；`ExpertPack` 是**给第三方**的
   产物契约。绑在指纹上，等于每次我们加个无关方法都逼所有外部 runtime 重新适配；不设版本，等于每次
   字段变化都静默破坏它们。因此：**新字段加 `pack_format`，递增它；`fp-n` 该涨照涨，两者不联动。**
2. **没有时间戳，导出是确定的。** 同一快照连续导出两次应当**逐字节相同**（实现须保证 `skills` /
   `connectors` / `members` 有序）。这样外部 runtime 可以直接拿 `provenance.content_digest` 当缓存键，
   也可以把两次导出做 diff 来回答「上游变了什么」。加一个 `exported_at` 会同时毁掉这两件事。

---

## 5. 外部 runtime 必须自己实现的东西（本方案的核心交付）

**这一节比 §4 重要。** 下面每一条在 allo 里都由 **Runtime 代码**保证；导出的数据里**没有**它们。
外部 runtime 若不实现，得到的是一个「看起来像团、实际不生效」的东西。

| # | 在 allo 里由谁保证 | 证据 | 外部 runtime 必须自己做的 | 不做的后果 |
|---|---|---|---|---|
| R1 | 成员存在、已安装、未停用 | `team_run.rs:167-207`（`agent_not_installed` / `agent_disabled`） | 建自己的构件注册表并做同样的**前置**校验（在开始编排**之前**） | 团长被委派给一个不存在的成员 |
| R2 | 成员池固定，模型不能动态增删 | `04` §4.1；`create_template` 冻结 participants | 把 `member_agent_ids` 当**硬边界**，不接受模型增删成员 | 模型自造成员＝越权 |
| R3 | 路由约束真的生效 | `04` §4.1「成员角色约束必须在 Runtime 代码中校验」；`routing_constraints` 只是**字符串**（`team_run.rs:249-256`、`:332`） | 把 `routing_constraints` 翻译成**自己的**可执行规则；译不动的部分**别声称支持** | 用户以为导出了 `software-company`，实际拿到一个不执行约束的假团 |
| R4 | 规划（Plan / DAG） | Leader 调 `nomi_delegate(strategy=planned)` → **服务端** `Planner/LlmPlanProducer`（`04` §4.3） | **自己实现编排**（形态自由，本方案不规定） | 没有这一步，就根本没有「团」——只剩一个人设 |
| R5 | Step 调度不变量 | `04` §4.4（依赖校验、`participant_index` 在池内、`max_parallel` 取 Team/调用方/Runtime 三者最小、终态不被迟到事件覆盖、retry ⇒ 新 Attempt、replan ⇒ 新 Plan Revision） | 定义并实现**等价**不变量 | 并发、重试、取消语义静默失效 |
| R6 | 模型解析优先级 | `team_run.rs:212-239`：preset 自己的 model ＞ 宿主默认；两者都无 ⇒ `team_member_model_unbound` | 自己解析；pack 只给 `declared` + `resolved` 两个**提示** | 成员跑不起来，或跑在与宿主预期不同的模型上 |
| R7 | 连接器栅栏 | `lib.rs:3990-4010`；声明了但被停用 ⇒ `connector_unavailable`，**不静默少绑** | 自己解析 `{id,name,enabled}`；自己配等价连接，或回调 `connector/call` | 专家静默地少了工具 |
| R8 | 技能挂载 | `lib.rs:3965-3981`（`include_skills` → `preset_enabled_skills`）；`team_run.rs:258` | 用 `skill/files` 取字节，按自己的机制挂载；并**自己决定**自动注入策略 | 专家带的技能全部不生效 |
| R9 | 工具 / 文件 / 网络 / 凭据权限 | `04` §3.2「由 Runtime Policy 计算，**不由 Persona 文本决定**」 | 自己算策略。`tools` / `disallowed_tools` 只是**声明**，不是权限边界 | 把 persona 里的一句「不要删文件」当成安全边界 |
| R10 | 运行级模型与思考等级 | `29`：`agent/run` 的 `model` / `reasoning_effort` 是**运行级**，优先级 显式 ＞ preset ＞ 宿主默认 | 自己定并实现优先级链 | 与宿主行为不一致，且用户无法解释差异 |
| R11 | 事件与 Artifact 规范 | `04` §5（事件类型表、Artifact 字段白名单） | 自己的事件/产物模型；pack 不带这些 | 外部消费者要额外适配一次 |

> **本文档的用法**：外部 runtime 的对接文档应当**逐条回应 R1–R11**，而不是只贴一份 `ExpertPack` 示例。
> 一张 N/A 的清单本身就是答案：它说明你选择不支持哪个语义，而不是让它在沉默中失效。

---

## 6. 协议面

### 6.1 两个方法

```text
agent/export { agent_id }              → AppServerExpertPack   (kind = "agent")
team/export  { team_id, team_version? } → AppServerExpertPack   (kind = "team")
```

- **拆成两个而不是一个 `expert/export`**：wire 里 `agent` 与 `team` 是两个 kind（与 `MentionKind`
  一致），协议词汇里**不存在** `expert` 这个词。「一个方法两种返回形状」会立刻逼出 `kind` 判别分支。
- **只有 WS 绑定，没有 HTTP 路由**（⚠️ **实现期订正，2026-09-24**）。方案原写「`GET /api/app-server/agents/{id}/export`」
  ——**那是错的**：`agent/list` · `agent/get` · `team/list` · `team/get` **四个方法一个 HTTP 路由都没有**，
  它们全在 `web/packages/client/src/http-transport.test.ts` 的 `DOCUMENTED_UNMAPPED` 里（实测：`lib.rs` 的
  `.route("/api/app-server…")` 表里没有任何 agents/teams 项）。导出属于**同一族**，所以同样只走 WS。
  代价与收益都很小：少写两个 handler，且计数从「映射 +2」变成「未映射 +2」（见 §7）。
- `team_version` 语义与 `team/run` 同：给了且不匹配 ⇒ `version_mismatch`。**实现位置有讲究**：校验放在
  **协议层**、对**包自己的 `version`** 比，而不是把 `team_version` 塞进 seam——包里本来就带着版本，
  协议层手上有全部所需信息，seam 因此保持成两个方法的纯读面。

### 6.2 闸门：`[expert_export]`——**默认开**的**减项表**（与 `[tools]` 同族）

```toml
# ~/.agent-store/config.toml（与 [tools] / [connector_proxy] 同址、同「只被 apps/agent-store 采纳」规则）
[expert_export]
enabled = true           # 缺省**开**；无此表 = 开
deny    = []             # 可选减法：这些 id 一律不出（agent id 或 team id）
```

env 覆盖 `AGENT_STORE_EXPERT_EXPORT`（JSON，同表形状，**整份替换**），与 `AGENT_STORE_TOOLS` /
`AGENT_STORE_CONNECTOR_PROXY` 完全同构——复用既有机制，不发明第二个。

**⚠️ 拍板结果与一次自我订正（2026-09-24，用户拍板「默认开」）**

本节早先按 `[connector_proxy]` 的形状写成 **fail-closed（默认关 + `allow`）**，理由是「persona 是策展内容」。
**那个类比是错的。** 仓库里真正的分类是**按表的用途**分的：

| 表 | 用途 | 失败方向 | 理由（原文） |
|---|---|---|---|
| `[connector_proxy]` | **授权**——让第三方在宿主的连接上**执行**工具 | **fail-closed** | 「缺表、缺 `enabled`、`allow` 为空、env 覆盖解析失败**一律拒绝**。一个手滑不能是「什么都不能调」与「什么都能调」的差别」（`24` §9.1） |
| `[tools]` | **减项**——只减去工具 | **fail-open** | 「坏文件最坏是「没减成」」（`24` §5.1） |
| **`[expert_export]`** | **读策展内容的字节**，不执行任何东西 | **fail-open（减项）** | 与 **`skill/files` 同级**——那面**根本没有门禁**，宿主接上 provider 就可用 |

所以正确的形状是 **`[tools]` 那一族**：默认开、只提供减法。

**已知且被接受的风险（写在这里，不留模糊）**：App Server 是本机服务，**任何本地客户端都能握手**
（`launchHarness({ client })`）并调用本面，因此默认开意味着**本机任意客户端可以批量拉走已装专家的
persona**。接受它的理由：

1. 它与 `skill/files` **同级**——那面今天就是这样，没有门禁；对本面单独加锁会造成
   「技能正文随便读、专家 persona 要解锁」的不一致。
2. **它不是保密边界**：同一台机器同一用户的进程本来就能直读
   `{work_dir}/agent-store-imports/<snapshot_id>/agents/*.md` 与 SQLite（`24` §4.4 定性在先）。
3. **拍板的理由本身是「调用方就是本机 owner」**——即授权层级与「能读到 SQLite 与快照目录」同级。
   在这个前提下，默认关**并不增加任何真实安全性**，只会让默认链路不可用（并要求每个用户去改配置）。
   需要收紧的运营者仍有一条一行配置的路（`enabled = false` 或 `deny`）。

**产品口径（据此关闭 §10.2 的待定）**：`license` 不作为门禁依据（读数见 §10.2：接进来覆盖率也约等于 0）。
许可问题由**运营者自行判断**；`[expert_export]` 只提供收紧能力，不代替法务判断。

**坏配置的后果**（与 `[tools]` 同样的失败模式，可接受）：解析失败 ⇒ **视为未减项，仍为开**。
理由同上——它只做减法，坏文件最坏是「没减成」。

### 6.3 能力位与错误码

- 能力位 `expert_export: bool`，`from_state` 取 `state.expert_packs.is_some()`。
  **⚠️ 实现期订正（2026-09-24）**：方案原写「被闸门二次约束时必须诚实（`enabled = false` 时报 `false`）」，
  **落地时按仓库既有范式改掉了**——`routes.rs:1101-1111` 对 `connector_calls` 的原文是「**无条件接线**，
  但 provider 的第一道闸门就是宿主策略……所以『接线』从不等于『可调』」。于是：
  - **能力位报的是「seam 接没接」**，不是策略。策略是**每次请求**的判定——`[expert_export]` 既可能被
    `AGENT_STORE_EXPERT_EXPORT` 覆盖，也可能在运行期改文件后重启生效；把它烘进握手期的能力位会制造
    第二个真相源。
  - `enabled = false` ⇒ **provider 已接线、闸门拒绝** ⇒ `policy_denied`；只有宿主**根本没接 seam** 时
    才是 `unsupported_operation`。两个码分工清楚：一个是「你关了」，一个是「这里没有这个面」。
  - **代价照实说**：客户端可能看到 `expert_export: true` 而后被 `policy_denied`，UI 不能只靠能力位灰按钮。
    这与 `connector_calls` 完全同形，是既有取舍，不是本次新引入的。
- 错误码：`policy_denied`（`enabled = false`，或 id 命中 `deny`）、`agent_not_installed`（含**团成员**未安装，
  与 `team/run` 同码同义）、`agent_disabled`、`preset_disabled`、`not_found`、
  `unsupported_operation`（宿主未接 provider）、`response_too_large`（包超 1 MiB，**不静默截断**）。
- `agent_not_installed` 的 `details` 必须**指名**是哪个成员，否则一个 10 人团只能得到「某些成员没装」。

### 6.4 成员未安装 ⇒ 整包失败，不产出残包

与 `team/get`（只回 id 列表，永远不会失败）不同，`team/export` 要展开成员定义，所以**要么全给、要么
明确失败**。产出一个「成员缺两个」的包，正是 §5 说的那种「看起来能用」的陷阱。

**与 `team/run` 的既有姿态一致**：`team/run` 对成员未安装 / 已停用也是**创建前硬失败**
（`agent_not_installed` / `agent_disabled`，`team_run.rs:167-207`），`05` §12.2 明写「成员与连接器的检查
都在创建时发生，**不会先开出一个残缺的 Leader 会话**」。导出沿用同码同义，不发明第二套。
**已知代价**：装了 9/10 个成员的用户会**完全导不出**——这与 `team/run` 的要求一致，不是本方案新加的
门槛（那个用户本来也跑不了这个团）。

### 6.5 物化：**站点配方 + 一支 live 脚本**，不做 SDK 助手（D8 第二半）

服务端只有 §6.1 的两个**只读**方法。同机消费者要「一个目录」时，走**一段 11 行的配方**（站点
`examples-sdk.md`），我们自己的端到端证明走**一支 `web/scripts/sdk-live-*.ts`**。

```ts
// 站点 examples-sdk.md 的配方：pack → 目录。消费者自己的代码，不是我们的 API。
const pack = await harness.agents.export(agentId);
await mkdir(dir, { recursive: true });
await writeFile(join(dir, "expert-pack.json"), JSON.stringify(pack, null, 2));
await writeFile(join(dir, "persona.md"), pack.persona.instructions);
for (const skill of pack.skills) {
  for (const file of (await harness.skills.files(skill.id)).files) {
    // file.path 是技能目录内的 POSIX 相对路径，服务端已做穿越校验（`24` §4.4）
    const target = join(dir, "skills", skill.name, file.path);
    await mkdir(dirname(target), { recursive: true });
    await writeFile(target, await harness.skills.readFile(skill.id, file.path));
  }
}
```

配方写出的**规范布局**（文档化的约定，不是 API 契约）：

> **⚠️ 配方必须自己决定「悬空引用」怎么办**（真机实测，见 §9.1 EX-008）。包里的技能是**声明**，
> 而「这个声明在本机能不能解析」是宿主事实：实测一个仓库夹具声明的 3 个技能，在 21 个已装技能里
> **一个都没有**。所以 `skill/files` 会对它们报错。**默认不该静默跳过**——把 `unresolved` 记下来交给
> 调用方，比让一个需求在沉默中消失好。脚本里的做法就是：`catch` → 记进 `dangling[]` → 继续。

```text
<dir>/
  expert-pack.json      # = 线上 pack 逐字节；含 pack_format
  persona.md            # = persona.instructions
  skills/<name>/…       # 可选：由上面那段循环补齐
```

**⚠️ 一次自我推翻（2026-09-24，用户拍板采纳）**：本节早先写的是 **SDK 公开面
`materializeExpert(harness, { agentId, dir, skills })`**。**砍掉它**，理由五条：

1. **它不覆盖任何新场景**。D8 要的是「同机 + 远端都能用」，而 **wire 本身就同时覆盖两者**——
   同机进程调 `agent/export` 与远端调一样容易。「同机工效」是**我加上去的需求**，不是用户的。
2. **它做的事全部由已有原语拼成**：`agents.export` + `skills.files` + `skills.readFile`
   （后者是 `24` §4.5 已交付的面）。11 行显而易见、完全基于公开原语的代码，**不值得一个带 semver
   承诺的公开 API**（`web/AGENTS.md` §2「不做投机性代码」）。
3. **布局是我们发明的，还没有消费者。** 一旦公开就得长期维护（改名 / 迁移要走站点 `upgrade`），
   而**任何外部 runtime 都要把它再翻成自己的布局**（`.claude/skills/`、`AGENTS.md`、自研注册表）——
   中间层不省集成的活，只省几次往返。
4. **砍掉它不影响 §2 第 4 条（引用而非内联）**：那条的理由是「消费方无论如何都要物化」，
   与有没有我们的助手无关。
5. **仓库有现成的中间形态**：`web/scripts/sdk-live-*.ts` 已有 10 个同类脚本——**可跑、可验收、
   不进 npm 包**。物化的端到端证明放那里正合适。

**已知代价**：D8 的第二半从**代码交付**变成**文档交付**（站点配方 + 一支 live 脚本）。
将来若真的出现第二个外部 runtime 在写同一段代码，那就是把它提升为 API 的证据——
**那时形状有依据，现在没有**。

> **为什么不做服务端物化**：`expert/materialize { id, dir }` 会把「调用方指定路径、宿主去写」变成
> 一个新的写面——路径穿越、越权写、「谁拥有那个目录」三个问题一起进来，而收益只是省掉客户端一次
> 写盘。同机场景下客户端本来就有写自己磁盘的全部权限，没有理由把这件事搬到服务端（§2 第 7 条）。

> **⚠️ 二次推翻（doc `35`，SDK 目录写入助手回来了）**：上面「不做 SDK 公开面」的拍板在 doc `35`
> （`35-sdk-expert-export.zh.md`）被**推翻**——`web/packages/sdk` 现在提供
> `exportAgent` / `exportTeam` / `materializePack` 三个函数，把本节的 11 行配方提升成了带 semver
> 的公开 API（`materializePack` = 把**内存里的 pack 对象连同它引用的技能字节写成一个真实目录**；
> team 形态新增 `members/<id>/persona.md`，技能跨成员按 id 去重；dangling 语义不变）。
> 推翻的触发与五条理由的逐条对账见 doc `35` §2.2；「第二个消费者出现了」正是本节预留的提升条件。
> **本节的其余内容仍然成立**：不做服务端写盘、不把配方搬进 `web/packages/client`（归属仍是 sdk）、
> 技能引用不内联；11 行配方降级为这三个函数的底层原理说明。

> **一条留给将来的边界**：这段配方**不得**被搬进 `web/packages/client`。`client` 刻意保持环境无关
> ——它的 base64 解码就明写「atob 优先、否则用 Buffer，以便在裸 Node 上也能跑」
> （`packages/client/src/skills.ts:68-73`），引入 `node:fs` 会把这个性质毁掉。要提升为 API 时，
> 归属是 `web/packages/sdk`（它已经拥有 `spawn.ts` 这类写盘能力）。**（2026-09-24 后记：这一条
> 已按 doc `35` 兑现，归属就是 `web/packages/sdk`。）**

---

## 7. 指纹与跨仓

新增 2 个方法（**都是 WS-only**，见 §6.1 的订正）＝**一次 wire 变更**，按 `web/AGENTS.md` §5 四步：

1. `fp-7` → **`fp-8`**；旧值全仓 grep 收尾，落点按 `scripts/check-protocol-fingerprint.mjs`
   的 `MIRRORS` 逐条覆盖（`web/scripts/mock-server.ts`、`web/scripts/smoke.ts`、
   `web/packages/sdk/src/readiness.test.ts`、`web/scripts/probe-agent-store-runtime.mjs` 等）。
2. 正文：`05` 头部指纹 + 新章节；`10` §7 错误码（**如需**新增码）；`README.md` 本行；本文档 §9。
   **不再有新的 SDK 公开面**（§6.5 已砍），所以 `07-typescript-sdk.md` / `12-sdk-packaging.md`
   **只需在客户端方法表里加两个方法**，不涉及包形状或发版口径的变化。
3. **跨仓 `C:\workspace\agent-store-site`**：`content/docs/{zh-CN,en-US}/typescript-sdk.md` §2 常量
   （中英各一处）、方法计数 **48 / 71 → 48 / 73**（**映射数不变**，两个新方法进 `DOCUMENTED_UNMAPPED`：
   本仓 `DOCUMENTED_ROUTE_SPLIT` 为 `48 / 23 → 48 / 25`）、`changelog` §4 未发布台账；`examples-sdk.md`
   补「导出专家给外部 runtime」一节（含 §5 的责任清单指针 + **§6.5 的 11 行物化配方**）。两语言结构一致。
   `bun run check:release-sync` 两边比对计数与版本锁步。
4. 完成标准：旧值在本仓代码里归零；`bun run check:fingerprint` 绿；`cargo test -p nomifun-app-server`
   与 `cd web && bun run typecheck && bun run test` 绿；站点 `check:docs-sync` **0 drift**。

**`pack_format` 不参与本流程**（§4.4）：它是产物契约的版本，不是 wire 指纹。

---

## 8. 实施顺序

```text
1. DTO（nomifun-api-types：AppServerExpertPack 一族 + lib.rs re-export）   → cargo check -p nomifun-api-types
2. seam（nomifun-app-server/src/catalog.rs：ExpertPackProvider trait、
        AppServerRouterState.expert_export；**不动 AgentCatalogProvider**）  → cargo check -p nomifun-app-server
3. adapter（nomifun-app/src/app_server_expert_export.rs：payload → persona、
        PresetService.resolve → model、安装状态 → connectors、递归团成员）    → adapter 单测
4. 闸门（agent_store.rs：`[expert_export]` **默认开** + `deny` + env 覆盖 + policy decide）→ 单测：默认开 / 关掉 / 命中 deny / 坏配置仍为开
5. 协议面（WS arm ×2 + HTTP 路由 ×2 + 能力位 + 错误码映射）                  → lib 测试 + e2e
6. TS 传输面（protocol.ts DTO + Capabilities；client `agents.export` / `teams.export`；
        路由表 +2 → 50；mock/smoke/计数守卫）                                → cd web && bun run typecheck && bun run test
7. live 验收脚本（**web/scripts/sdk-live-expert-export.ts**：装专家 → export → 按 §6.5 配方
        物化目录 → 断言 persona 与 skill 文件逐字节自洽）                      → EXIT=0 + 读数回写 §9.1
8. 指纹 + 正文（05 / 10 / 07 / README / 本文 §9）+ 跨仓站点（含 examples-sdk 配方）→ 旧值归零；站点 0 drift
```

**第一道真实性闸门是第 3 步**：它要证明 persona 真的是「payload 里那份 Markdown body」，
而不是靠 preset 反查出来的近似物。用一条**长于 1200 字**的正文做对照（`skill/get` 的截断教训）——
pack 里的 `instructions` 必须**完整**，且与该快照磁盘上的 `agents/*.md` 正文逐字相等。

---

## 9. 测试与验收

| 层 | 用例 |
|---|---|
| 闸门 | `[expert_export] enabled = false` ⇒ `policy_denied`；id 命中 `deny` ⇒ `policy_denied`；**缺表 ⇒ 开**（断言默认值本身就是用例）；env 覆盖解析失败 ⇒ **仍为开**（减项表 fail-open，与 `[tools]` 同） |
| 前置 | 未 `install/*` ⇒ `agent_not_installed`；`install/disable` 后 ⇒ `agent_disabled` / `preset_disabled`；宿主未接 provider ⇒ `unsupported_operation` |
| **反例 1（最重要）** | `agent/list` · `agent/get` · `team/get` · `store/list` 的**响应 JSON 里不含** 该专家 `instructions` 的任何子串。这条证明目录面没被顺手污染——没有它，「加个字段」的诱惑会在下一个人手里重新出现 |
| **反例 2** | pack 的序列化结果里不含任何 transport / env / headers / token 的值（照 `24` §5.5 的凭据反例写法，断言式） |
| 正文保真 | 用 >1200 字的 persona：`agent/get` 拿不到正文、`agent/export` 拿到**完整**正文，且与快照磁盘上的 `agents/*.md` body 逐字相等 |
| 确定性 | 同一快照连续导出两次**逐字节相同**（无时间戳；列表有序） |
| 团 | 成员递归展开、leader 在首位；某个成员未安装 ⇒ **整包失败**且 `details` 指名该成员 |
| 尺寸 | 包 > 1 MiB ⇒ `response_too_large`，不截断 |
| **物化（live 脚本，不是 API）** | `web/scripts/sdk-live-expert-export.ts` 走完 §6.5 配方后：`expert-pack.json` 逐字节等于线上 pack；`persona.md` 等于 `persona.instructions`；每个技能目录**逐字节等于** `skill/file` 的返回；重复跑结果相同 |
| 无新 SDK 公开面（历史口径，doc `32` 落地时） | `web/packages/sdk/src/index.ts` 与 `web/packages/client/src/index.ts` 的导出集合**不含**物化相关符号；`client` 源码与产物**不含** `node:fs` / `node:path` |
| SDK 公开面（现行口径，doc `35` 推翻后） | `sdk` 导出集合**含且仅含** `exportAgent` / `exportTeam` / `materializePack`（+ 类型）这批新符号；`client` 导出集合**仍不含**目录写入符号、源码与产物**仍不含** `node:fs` / `node:path`（该约束从未松动） |
| 指纹/跨仓 | `bun run check:fingerprint` 绿；本仓与站点方法计数一致；站点 `check:docs-sync` 0 drift |

### 9.1 落地记录

#### 后端（2026-09-24，步骤 1–5 已落，步骤 6–8 未做）

分支 `feat/expert-pack-export`，基线 `94553ce4a`（本地 main；`origin/main` 那 25 个提交与本次**零文件重叠**，
且一个都不碰本方案的目标文件——实测）。

| 层 | 实际改动 | 读数 |
|---|---|---|
| DTO | `nomifun-api-types/src/app_server.rs`：`APP_SERVER_EXPERT_PACK_FORMAT` + 12 个 `AppServerExpert*` 类型；`lib.rs` re-export | `cargo check -p nomifun-api-types` ✓（29.7s） |
| seam | `nomifun-app-server/src/catalog.rs`：`ExpertPackProvider` trait（2 方法）、`ExpertPackError`（6 变体）、`MAX_EXPERT_PACK_BYTES = 1 MiB` | `cargo check -p nomifun-app-server` ✓ |
| 状态/能力位 | `AppServerRouterState.expert_packs` + `Default`；`CapabilityAvailability.expert_export` / `Capabilities.expert_export` | 见 §6.3 的订正 |
| 闸门 | `agent_store.rs`：`AgentStoreExpertExport` 表 + `ExpertExportPolicy`（**默认开**，`decide()`）；`services.rs`：`EXPERT_EXPORT_ENV` + `resolve_expert_export_policy` + `AppServices.expert_export_policy` | 与 `[tools]` 同族（fail-open） |
| adapter | 新 `nomifun-app/src/app_server_expert_export.rs`（含 `ExpertPresetReader` seam，镜像 `PresetRegistrar`）；`app_server_importer.rs` 的 4 个 payload 读取器改成 `pub(crate)` 复用 | `cargo check -p nomifun-app` ✓ |
| 接线 | `routes.rs`：`expert_packs` **无条件接线**（闸门在 provider 里，照 `connector_calls` 原文） | 同上 |
| 协议面 | WS arms ×2（`agent/export` / `team/export`）、`expert_pack_error` 映射、`export_agent_impl` / `export_team_impl`（含 `team_version` 校验）；**无 HTTP 路由** | `cargo check -p nomifun-app-server` ✓ |
| 单测 | 新增 6 条：能力位独立性、未接 seam 关闭、往返、`team_version` 守卫（含空白串=未指定）、6 个错误码各自的稳定码、**persona 只在 pack 上而不在目录面上** | `cargo test -p nomifun-app-server --lib` → **173 passed / 0 failed**（本次前 167，新增 6 条全绿） |

**与方案的偏差（2 处，均为实现期发现，已就地订正正文）**：

1. **`agent/export` / `team/export` 没有 HTTP 路由**（§6.1）。方案原写了两个 `GET` 绑定——**错的**：
   `agent/list` · `agent/get` · `team/list` · `team/get` 四个方法一个 HTTP 路由都没有，全在
   `DOCUMENTED_UNMAPPED` 里。于是计数是「未映射 +2」而非「映射 +2」：本仓 `48 / 23 → 48 / 25`，
   站点 `48 / 71 → 48 / 73`。
2. **能力位报 seam、不报策略**（§6.3）。方案原写「闸门关着就报 `false`」，落地时按
   `connector_calls` 的既有范式（`routes.rs:1101-1111`「无条件接线……『接线』从不等于『可调』」）改成
   能力位只反映 seam 是否接线；`enabled = false` 走 `policy_denied`。**代价已写进 §6.3。**

**一处实现选择（非偏差，记录理由）**：adapter 引了 `ExpertPresetReader` 这个窄 seam，而不是直接吃
`Arc<PresetService>`——理由是 `PresetService` 是具体结构体，直接依赖会让「哪些 preset 状态可导出」
只能靠起真服务来验证。这与 `app_server_installer.rs` 的 `PresetRegistrar` 是同一手法。

**团队包的两处刻意留空**（写下来免得被当成漏做）：`team.preset_revision` 恒为 `None`、`team.model` 恒为
全 `None`。理由：团队自己的那份 Preset 只是安装器为「让 `team/run` 有东西可解析」造的产物，**不是定义
的一部分**；团队没有自己的模型，模型在成员身上。团队的漂移由成员包承载。

#### TS 客户端与指纹/跨仓（2026-09-24，步骤 6 与 8 已落）

| 层 | 实际改动 | 读数 |
|---|---|---|
| TS DTO | `web/packages/protocol/src/protocol.ts`：`APP_SERVER_EXPERT_PACK_FORMAT` + `ExpertPack` 一族 11 个类型（含 `ExpertTeamPack`）；`Capabilities.expert_export`（**注释写明它报的是 seam、不是策略**） | `cd web && bun run typecheck` **exit 0** |
| TS 客户端 | `client/src/agents.ts` 加 `export(agentId)`、`client/src/teams.ts` 加 `export(teamId, teamVersion?)`；两处类注释改写为「目录面 vs 导出面」 | `cd web && bun run test` → **513 passed / 1 skipped**（与改动前同数） |
| 计数守卫 | `http-transport.test.ts`：`DOCUMENTED_ROUTE_SPLIT` `48 / 23 → 48 / 25`，两个新方法进 `DOCUMENTED_UNMAPPED` | 同上（该断言就是计数守卫本身） |
| 指纹 | `fp-7` → **`fp-8`**，本仓 **9 处 / 7 文件**（`lib.rs` authority、`protocol.ts`、`http-transport.ts`、`mock-server.ts`、`smoke.ts`×2、`readiness.test.ts`×2、`probe-agent-store-runtime.mjs`）+ 站点 2 文件；两处常量注释补了 `fp-8` 段落（**保留 `fp-7` 的历史描述，不改写**） | `bun run check:fingerprint` → **✓ fp-8 一致：本仓 10 落点 / 7 文件 + 站点 2 文件** |
| 跨仓站点 | `typescript-sdk.md`（中英）：§2 常量 `` `"fp-8"` ``、两处方法计数 `48 / 73`、§5.3 的未映射清单 `23 → 25` 并补两个方法名；`changelog.md`（中英）§4 台账重排为「两批、最新在前」，新增 `fp-8` 破坏性条目并订正原文里「指纹仍是 fp-7」的过期说法；`examples-sdk.md`（中英）新增 **§9.1 导出专家给外部 runtime**（含 11 行物化配方、三个错误码的分工、WebSocket-only 说明） | 站点 `check:docs-sync` **10 页 / 2 语言 / 0 drift**；`test:docs-sync` **16 pass / 0 fail**；本仓 `bun run check:release-sync` **✓ 48 / 73 两边一致** |
| 正文 | `05` 头部指纹 + 新增 §4.1.1 / §4.2.1；`07` 的 `AgentClient` / `TeamClient` 加 `export` 与一段说明；`README.md` 索引行 | — |

**⚠️ 跨仓操作的一个坑（记下来）**：站点仓里 `48 / 71` 与 `fp-7` 也出现在 **`changelog.md` 的历史条目**里
（描述已发布的 beta.5）。所以**不能整仓批量替换**——只能改「当前值」那几处（`typescript-sdk.md` 的
§2 常量与 §5.3 两处计数），历史条目一律不动。另一个坑：`cd` 对 .NET 静态 API
（`[System.IO.File]::ReadAllText`）**不生效**（它看 `[Environment]::CurrentDirectory`，而 `Set-Location`
不改它），第一次批量替换因此静默什么都没写；校验输出暴露出原值仍在才发现。**改文件一律用绝对路径。**

#### 门禁读数，以及一个与本次无关的上游红灯

按 **CI 的 `Repo gates` job 真实执行的那张清单**逐条跑（13 条）——**全部 ✓**：
`check:i18n` · `check:theme` · `check:icons` · `check:codemirror-runtime` · `check:error-surface-contract` ·
`check:support-surface-contract` · `check:windows-console-hide` · `check:process-runtime-boundary` ·
`check:browser-platform-boundary` · `check:market` · **`check:fingerprint`** · **`check:release-sync`** ·
`help --check`。

**`bun run check`（聚合）当前是红的，但红的不是本次改动**——它在 `ui/` typecheck 上失败，报错全在
`ui/src/renderer/pages/videoCanvas/oc/…` 与 `ui/src/renderer/utils/analytics/updateTelemetry.ts`。
三条证据说明这是**上游既有缺陷**：

1. `git show origin/main:ui/src/renderer/utils/analytics/updateTelemetry.ts` 里 `getUpdateCdnHost`
   **在 L8 与 L42 各 import 一次**——重复标识符，真实缺陷；
2. `git diff --name-only origin/main...HEAD -- ui/` **为空**：本分支一个字都没碰 `ui/`；
3. CI 的 `Repo gates` job **不包含 `ui` typecheck**（它只跑上面那 13 条），所以这个红灯不进 CI。

**处理方式：不做**。`web/AGENTS.md` §3「只清理自己制造的混乱」——这是别人那条线的 `ui/` 缺陷，
登记在此供接手者处置，不在本次改动范围内。

**同类的第二道红灯：`check:agent-vocabulary`。** 它同样**不在 CI 的 `Repo gates` 里**（AGENTS.md 把它
算在 `bun run check` 的 ui 前端检查那一串），且**改动前就已经是红的**（26 处既有：`nomi-agent` 的
`isolated_subagent`、`fp-5` 的 `orchestration` 文案、`nomifun-mcp` 与 `docs/architecture`）。

**⚠️ 但它抓到了我写的一处**：`app_server.rs` 的 `AppServerExpertPack` 注释里我用了 "Team
**orchestration**" —— 而 `orchestrat` 是退休词（`scripts/check-agent-vocabulary.mjs:30-31` 的正则：
`orchestrat|sub[-_ ]?agent|agent[-_ ]?cluster|\bfleet…`）。**已改**为「how a team plans and schedules its
steps」，`web/packages/protocol/src/protocol.ts` 与站点 en `examples-sdk.md` 的同句一并改掉。订正后
违规数 **27 → 26**，**我引入的 0**。

**这一条值得记住**：改 wire 面时会大量写「这个包里**没有**什么」，而描述「没有什么」最容易顺手用上
被退休的词。写完之后跑一次 `check:agent-vocabulary` 是必要动作——它在 `bun run check` 里，但**不在 CI 里**，
所以 CI 绿不代表它绿。

#### 基线处置：合并了 `origin/main`

本分支原本基于本地 `main`（含并行会话的 `94553ce4a`），而 `origin/main` 另有 25 个提交。
开工前实测**两条线零文件重叠**、且 origin 那 25 个**不碰本方案任何目标文件**，所以直接
`git merge origin/main`：**0 冲突**。合并后上面 13 条门禁全绿。

#### adapter 级单测（2026-09-24，10 条，真内存 SQLite）

夹具用 `nomifun_db::init_database_memory()` + `SqlitePluginSnapshotRepository`（真仓储、真 CHECK 约束），
Preset 侧用 `ExpertPresetReader` 的窄 fake。**10 条全绿**（`cargo test -p nomifun-app --lib export`）：

| 用例 | 钉住什么 |
|---|---|
| `persona_is_the_payload_body_verbatim_even_when_long` | 1300 字正文**逐字**进包且不被截断；fake 的 Preset `instructions` **故意是不同的串**——若 adapter 改成读 Preset，这条立刻红 |
| `export_is_byte_identical_across_calls` | 同一状态两次导出**逐字节相同**（无时间戳、列表有序） |
| `the_roster_expands_leader_first_and_without_duplicates` | 声明列表原样回报；展开**团长在前**、同名去重；`team` 无 `preset_revision`／无 model（团队自己的 Preset 不进定义） |
| `a_team_with_an_uninstalled_member_fails_as_a_whole_and_names_it` | 缺一成员 ⇒ **NotInstalled 并含该成员 id**（断言的是「消息里点名」，不是「报错就行」） |
| `installation_state_decides_not_installed_and_disabled` | 未装 ⇒ `NotInstalled`；**并且**钉住「关掉的权威信号是 Preset、不是组件行的 `disabled` 列」 |
| `a_switched_off_preset_reports_disabled_not_missing` | Preset 关掉 ⇒ `Disabled`（与「没有这个东西」分开） |
| `the_gate_runs_before_any_read` | **空仓库 + denied id ⇒ `PolicyDenied`**——若先读库就会答 `not_found`，所以这条证明闸门在最前；另测 `enabled = false` 关整面 |
| `an_unknown_id_is_not_found_and_the_message_names_it` | `NotFound` 且消息带 id |
| `connectors_come_from_the_install_state_not_the_manifest` | 只报**装好且启用**的连接器；装后被停用 ⇒ 不再作为依赖出现 |
| `a_pack_carries_no_connector_credential_and_no_absolute_path` | **对抗式**：把 `SECRET-…` 哨兵、`npx`、`"env"` 真的种进连接器 payload 与快照的 `source_uri`，再断言包的序列化结果里**一个都没有** |

**实现期订正（第 3 处，与方案无关但与 §6.3 的语义有关）**：写第 5 条时我先按「组件行的 `disabled = 1`」
构造，测试红了——**而红得对**。adapter 读的是 **Preset 的 enabled**，这与 `team/run`（`team_run.rs:192-207`）
逐字一致；`install/disable` 两个都写（`app_server_installer.rs:353-358` 与 `:956-963`），所以实践上一致，
但只有 Preset 是引擎真正解析的那一处。**读组件行会接受一个 `agent/run` 会拒绝的专家**，所以是测试写错了，
代码是对的——现在这条测试把这个权威关系钉住了。

**顺带发现并订正**：`05` §4.2.1 我原先把 `connectors` 写在 `team { … }` **里面**。实际在**顶层 pack**上
（agent 恒空、team 放该团快照的连接器），DTO 一直是对的，是文档写错了。

#### 真机验收（2026-09-24，步骤 7）

`web/scripts/sdk-live-expert-export.ts` 起一支**独立 data-dir、独立端口**的一次性宿主（`launchHarness`
自己 spawn，跑完 `server.close()`），导两个**仓库既有夹具**（`file-paths` 供物化循环、`software-company`
供 5 人名单与连接器），**RESULT PASS（9/9）**：

| 判据 | 实测读数 |
|---|---|
| EX-001 能力位 | `expert_export: true`（同一次握手里 `skill_files` / `connectors` 等一并为真） |
| EX-002 正文保真 | `agents.get` 的键里**没有** `instructions` / `persona`；`agent/export` 的 persona 与夹具 `agents/lead-agent.md` 的 body **21 字符、`exact: true`** |
| EX-003 确定性 | 同一专家两次导出 **717 字节，逐字节相同** |
| EX-004 技能是引用 | `pack.skills=[{id:"hello",name:"hello"}]`；`skill/files` 列出 `SKILL.md`，`skill/file` 回 **83 字节**，与磁盘上的夹具文件**逐字节相同** |
| EX-005 §6.5 配方 | 目录写成：`expert-pack.json` **917 字节且与线上 pack 逐字节相同**、`persona.md` 21 字符与 `persona.instructions` 相同、`skills/hello/SKILL.md` 与 `skill/file` 的返回相同 |
| EX-006 小团顺序 | `file-paths`：声明 1 名、展开 **2** 名，**团长在首位**，两名成员各有正文；`preset_revision` / `model.resolved` 缺席 |
| EX-007 5 人团 | `software-company`：声明 4 名、展开 **5** 名，团长在首位 |
| EX-008 悬空引用 | 该夹具声明 `planning` / `requirements` / `coding`，本机 **21 个已装技能里一个都没有** ⇒ 包**如实上报 3 个声明**（见下） |
| EX-009 连接器按安装态 | `team/get` 的连接器 = 包里的 = `01a0bd8a-a54e-…`；用**协议面** `install/disable` 停掉该组件 ⇒ 包变 `[]`；`install/enable` 恢复 ⇒ 回到原值 |

**EX-008 是一条真实的产品性质，不只是夹具巧合**：该夹具声明的三个技能在夹具里**都不存在**
（夹具自带的是 `release-notes` / `review-checklist`）。包报的是**声明**，而「这个声明在本机能不能解析」
是**宿主事实**。这与 `agent/run` 的失败关闭一致（声明了却取不到 ⇒ `PRESET_SKILLS_UNAVAILABLE`），
所以**如实上报是对的**——静默丢掉会把一个需求藏起来。**代价**：§6.5 的配方必须自己决定遇到悬空引用
怎么办；脚本里的做法是**记录下来并继续**，不静默跳过。

**实现期发现的 2 处（都是我脚本的断言写错，代码是对的）**：

1. **`None` 是「字段缺席」，不是 `null`。** `skip_serializing_if = "Option::is_none"` 让
   `preset_revision` / `model.resolved` 在为空时**整个键消失**，到 TS 侧是 `undefined`。我写了 `=== null`
   于是判据假红。**这一条对消费方是真陷阱**：`pack.provenance.preset_revision ?? null` 才是安全读法。
2. **「新装的 MCP server 行默认 `enabled = false`」在这条路径上不成立。** `sdk-live-team-leader.ts`
   的注释是那样写的（并据此调第一方 toggle 路由），但本次实测：`install/run` 之后该连接器**已经是启用**状态，
   包一开始就报出了它，而 toggle 返回 **409**。所以 EX-009 改成用**协议面**的 `install/disable` /
   `install/enable` 双向改状态——比调第一方路由更贴协议，判据也更强（**两个方向都验**）。

**清理复核**：脚本跑完 `agent-store` 进程 **0 个**、临时端口**无监听**、物化目录已 `rm`。
SDK 自己 spawn 的 data-dir 由 `close()` 删除（本次两次 `UsYjqs` / `XqKcLw` 都已不在）。
`%TEMP%` 里另有一个 `agent-store-sdk-cYxsmj`（创建于 **09-17 16:18**）——**不是本次的**，
是更早会话留下的，按「不清理别人制造的混乱」**未动**。

---

## 10. 开放项

1. **命名定稿（本节给建议，正式定稿要先进 `10-public-contracts.md`）**。
   顺序按 `10` §8：「公共枚举和字段先修改本文」→ 同步 schema / SDK 类型 / 测试 → 其他文档只引用。

   | 名字 | 建议 | 理由 / 必须先写下来的坑 |
   |---|---|---|
   | `agent/export` · `team/export` | **保留** | 动词与资源匹配，与 `agent/get` / `team/get` 同族。**但有一处历史碰撞必须显式记录**：`24` §2 里被**否决**的方案恰好叫 `connector/export`（「等于把宿主凭据交给第三方进程」）。本处的 `export` 导出的是**定义**（persona / 模型 / 引用），被否决的是**凭据**（transport / env / headers / token）——同名不同物。不写清楚，将来一定有人拿 `24` 来质疑这两个方法 |
   | `[expert_export]` + 能力位 `expert_export` | **保留** | 与 `[connector_proxy]` ↔ `connector_calls` 的「表名 ↔ 能力位」对偶一致。注意它现在是**默认开的减项表**（§6.2），不是授权表 |
   | `ExpertPack` · `pack_format` | **保留**（次选 `ExpertDescriptor`） | **`expert` 不是发明**：`AppServerAgentDetail.expert_type` **已经在 wire 上**（`app_server.rs:515`，无 serde rename），官方市场 id 也叫 `experts`；且 `kind` 仍是 `"agent" \| "team"`，没有引入第三种 kind。**要知道的风险**：本仓 `pack` 另有一义——`release:pack` / `pack:market` / `pack-market-zips.mjs` 全是「打成归档（zip/tarball）」，所以这个名字有被误读成「一个 zip」的余地。要零歧义就整体改成 `ExpertDescriptor` / `descriptor_format`。**不要用 `manifest`**：本仓 `manifest` 已被 `plugin.json` / `marketplace.json`（清单）占用 |
   | `persona.instructions` | **保留** | 与 `Preset.instructions` / `CreatePresetRequest.instructions` 逐字同源，不发明第二个词 |
   | `provenance` / `runtime_binding` | **保留** | 前者承载 `source` / `snapshot_id` / `content_digest`，后者诚实声明 `{runtime, portable}` |
   | SDK 面 | `harness.agents.export()` · `harness.teams.export()`（仅此两个） | 零跳，符合 `31` 定下的入口形状与 §4.4「子客户端保留资源命名空间」。**不再有 `materializeExpert`**（§6.5） |

   **定稿的完成判据**：`10` 先落文 → `05` / `07` / 站点三处同步 → 旧建议名在正文里只剩本文这张表。
2. ~~**内容授权**~~ → **已定，不再是开放项**（2026-09-24 用户拍板「默认开」，口径记在 §6.2）。
   读数存档（供将来复核）：`PluginManifest` **没有** `license` 字段，且该结构**没有**
   `deny_unknown_fields`（`manifest.rs:122-124`）→ 未知键被**静默丢弃**，`license` **从不进入**
   组件 payload；即「不是市场里没有，而是**我们没接**」。而真实市场**确实携带**它——`18` §11 D7 的普查里
   manifest 层 spec-silent 字段含 `license`，**experts 市场只有 ×1 条**（`18-marketplace-spec.zh.md:365`）。
   结论：**即使接进来覆盖率也约等于 0，不能作为闸门依据**，因此闸门是运营者的显式减项能力
   （`enabled = false` / `deny`），许可判断留给运营者。**本条的实质结论保留**：这不是数据能自动回答的问题。
3. **专家**没有**连接器依赖（这不是缺口，是格式语义）**：`02` §5.1 明确——插件级 `mcpServers`
   是**插件启用后的能力**，官方出于安全原因**不自动成为每个 Agent 的权限**；导入器必须记
   `ignored-by-source-runtime`，**不得**把它当作 Agent 级授权（`02-codebuddy-workbuddy-import-spec.md:127`）。
   所以 pack 里专家的 `connectors` **恒为空数组是忠实的**，而**不是**「今天没接上」。
   > **曾经写错并已订正（2026-09-24）**：本条目早先写作「专家的连接器依赖今天不存在，要不要顺手补上」。
   > 那是错的——补它等于**发明导入从未建立的授权**，与 `app_server.rs:550-554` 对 Team
   > `connectors` 的既有口径直接冲突。**该项现为非目标**，不是待办。
4. **专家团今天没有真实素材**：官方市场**实测 0 条 team**（`sdk-live-team-leader.ts:76` 的判据，
   490 条 = 262 skill + 228 connector）。所以 §9 的团用例只能用仓库夹具 `fixtures/software-company`。
5. **`07-typescript-sdk.md` 的 SDK 面**：只有 `harness.agents.export()` / `harness.teams.export()`，
   命名要与 `31` 定下的入口形状一致（`Harness` 扁平转发，零跳），不要在子客户端上另立风格。
   **本次不新增任何 SDK 公开符号**——物化是站点配方（§6.5），不是 API。将来若提升，归属是
   `web/packages/sdk`，**绝不能进 `client`**（`client` 刻意环境无关，见 §6.5 末条）。
6. **服务端落盘契约：不做（D8 已定）**。同机消费者拿到目录这件事，改由**站点配方 + 一支 live 脚本**
   （§6.5）引导消费者在自己的代码里完成——服务端保持**只有两个只读方法**。留在 §2 第 7 条里的理由是
   安全面（路径穿越 / 越权写 / 目录归属），不再重复。**注意**：persona 的原始字节本来就在
   `{work_dir}/agent-store-imports/<snapshot_id>/agents/*.md`，所以「同机能不能拿到」从来不是技术问题；
   §6.5 解决的是**让它变成受支持、可复现、带 `content_digest` 的一次操作**。
