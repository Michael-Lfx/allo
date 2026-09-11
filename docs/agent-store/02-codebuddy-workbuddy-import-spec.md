# CodeBuddy / WorkBuddy 导入规范

> 状态：✅ 已实现（nomifun-importer，2026-08 迭代落地 Phase 1）；市场来源（GitHub/Git/HTTP）与导入后运行时激活留待后续
> 日期：2026-08-26
> 前置：`00-architecture-decision.md`、`01-domain-model.md`
> 依据：https://www.codebuddy.cn/docs/cli/plugins、/plugins-reference、/plugin-marketplaces、/sub-agents、/agent-teams（官方文档已提取正文部分）

> 实现记录：V1 支持本地目录四类来源（`codebuddy-plugin` / `workbuddy-skill-market` /
> `workbuddy-connector-market` / `workbuddy-cli-connector`）。导入流程、路径安全（§7/§11.1）、
> 幂等与 digest 冲突（§9）、部分失败（§11.2）、凭据只建 Schema（§10）均已按本规范落地；
> 验收结果见 `02-codebuddy-workbuddy-import-spec.md` §14。

## 1. 导入范围

| 来源 | 位置 | 对象 |
|---|---|---|
| CodeBuddy/WorkBuddy 插件 | `.codebuddy-plugin/plugin.json` + 组件目录 | PluginSnapshot |
| WorkBuddy Skill 市场 | `.codebuddy-skill/marketplace.json` + `skills/<slug>/` | SkillDefinition |
| WorkBuddy Skill 单目录 | `skills/<slug>/SKILL.md`（无 marketplace.json） | SkillDefinition |
| WorkBuddy Connector 市场 | `.codebuddy-connector/connectors.json` + `connectors/<slug>/` | ConnectorDefinition + CredentialSchema |
| CLI 连接器（单目录） | `connectors/<id>/cli.json` + `skills/<slug>/SKILL.md` | ConnectorDefinition（cli）+ SkillDefinition（随附技能） |

约束：导入器只把来源当作输入格式，不要求 allo 原生解析器理解来源字段；标准输出为不可变 PluginSnapshot 与标准化定义。

> 注 1：CLI 连接器目录（`cli.json` + `skills/`）是真实 CodeBuddy 市场的布局（wecom / feishu /
> tmeet 等）。`cli.json` 本身不携带身份（name/version 均无），V1 用**目录名**作为插件身份，
> 无版本声明时以 `1.0.0` 参与快照版本（§4 默认值规则）。随附 `skills/` 下的每个
> `SKILL.md` 解析为 SkillDefinition（02 §5）。

> 注 2：单 Skill 目录（`skills/<slug>/`）没有 marketplace.json——真实市场根与「单 skill 目录」
> 共用 `workbuddy-skill-market` 来源类型：目录根含 `SKILL.md` 即视为单个 skill，身份 = 目录名
> （02 §4 默认值规则）。这样用户可粘贴任意市场根或单个 skill 目录路径。

## 2. 导入流程（固定顺序）

```text
1. 定位来源（插件根 / 市场清单 / 连接器目录）
2. 解析 Manifest（plugin.json / marketplace.json / connectors.json）
3. 路径校验：相对路径、拒绝 ../ 越界、拒绝外部符号链接逃逸
4. 解析路径变量（见 §7）
5. 复制到版本化不可变缓存，计算 content_digest
6. 生成 PluginSnapshot（含 components、provenance、compatibility_report）
7. 按组件类型产出标准化定义
8. 对不支持的组件标记状态，绝不静默丢弃
9. 注册到 Catalog（本地库：SQLite 为桌面 MVP 首选）
```

## 3. 插件目录结构（官方文档已证实）

```text
plugin-root/
├── .codebuddy-plugin/
│   └── plugin.json          # 清单；目录内通常只放清单
├── commands/                # Markdown 命令（/plugin:command）
├── agents/                  # Markdown 子代理定义
├── skills/<name>/SKILL.md
├── hooks/hooks.json
├── .mcp.json
├── .lsp.json
├── bin/ scripts/            # 可执行/脚本（插件启用后可进 PATH）
├── settings.json / output-styles/ / themes/ / monitors/
```

注意：`commands/ agents/ skills/ hooks/` 位于插件根，不在 `.codebuddy-plugin/` 内。

## 4. plugin.json 字段与处理

| 字段（官方文档确认） | 导入处理 |
|---|---|
| `name`（唯一必填） | 插件身份 + Skill 命名空间来源 |
| `version` / `description` / `author` | 元数据；version 参与快照版本（`author` 兼容字符串与 `{"name","email"}` 对象） |
| `agents` | 目录或文件路径引用（相对、`./` 开头；兼容字符串单值与数组）→ AgentDefinition |
| `skills` | 目录或文件路径引用（同左）→ SkillDefinition |
| `commands` | 目录或文件路径引用（同左）→ CommandDefinition |
| `hooks` | 文件/目录引用 → LifecycleHookDefinition（`hooks` 兼容对象与字符串值——字符串仅保留元数据） |
| `mcpServers` / `.mcp.json` | 插件级 MCP 配置 → ConnectorDefinition |
| `lspServers` / `.lsp.json` | → LspDefinition（V1 元数据级；兼容字符串值——仅保留服务器名清单） |
| `userConfig` | 配置 schema → CredentialSchema；敏感项进安全存储 |
| `dependencies` | 插件依赖（数组或 `{"connectors":[...]}` 对象形式；对象展开为条目并带 `group` 标记）→ PluginDependency |
| `defaultEnabled` / `channels` | 安装/启用策略元数据 |

规则：

- Manifest 路径一律相对于插件根，必须以 `./` 开头；自定义组件路径替换默认目录，除非默认目录也列入数组；
- `name` 同时是 Skill 命名空间（如 `my-plugin:hello`），导入后保留该命名空间语义（`plugin:skill` 形态）；
- Agent 与 Skill 共用组件 id 命名空间；同名时（如 `aihot` 插件含 agent `aihot` + skill `skills/aihot`）Skill 自动加 `-skill` 后缀消歧，不丢组件。

## 5. 组件映射总表

| 来源组件 | 标准化对象 | 导入动作 |
|---|---|---|
| `agents/*.md` | AgentDefinition | 解析 frontmatter + 正文为结构化 persona/策略字段（见 §5.1） |
| `skills/*/SKILL.md` | SkillDefinition | 保留正文/frontmatter/references/scripts/templates/assets；`$ARGUMENTS` 说明保留 |
| `commands/*.md` | CommandDefinition | 用户可调用 prompt/skill 定义（`plugin:command`） |
| `.mcp.json` / `mcpServers` | ConnectorDefinition | 命名空间化 MCP 工具；**插件级** MCP = 插件启用后能力，不自动成为每个 Agent 的权限 |
| `hooks/hooks.json` | LifecycleHookDefinition | 事件 + 匹配器 + action（command/http/prompt/agent）；V1 只导入与静态验证，默认不执行 |
| `.lsp.json` | LspDefinition | 元数据级；不伪装成 MCP 工具 |
| `userConfig` | CredentialSchema | 字段级 schema；敏感值仅引用安全存储 |
| `dependencies` | PluginDependency | 依赖解析；跨市场依赖默认禁止，需显式 allowlist |
| `bin/` / `scripts/` | 受控 CLI/Workflow 能力 | 不自动执行；不暴露任意 shell |
| `themes/ settings.json output-styles/ monitors/` | metadata-only | 直到对应宿主能力落地 |
| `teamInfo`（WorkBuddy 扩展） | AgentTeamDefinition | V1 导入固定成员、Leader 引用和 Team 策略；不创建 ExecutionTemplate，由 Runtime Adapter 在 TeamRun 创建时物化；支持 planned DAG、局部并行、事件、重试和 replan；不要求完整 Mailbox | 见 §6 |

### 5.1 agents/*.md 字段保留清单

Frontmatter 解析（`frontmatter.rs`）对真实市场文件做了兼容处理：
- **CRLF 行尾**（`\r\n`）：透明归一化后再解析；
- **非严格 YAML**（未加引号的引号、内嵌 JSON 字符串、折叠标量）：严格 YAML 失败时回退到
  **宽松行级解析**——只提取顶层 `key: value` 标量（name/description/version/homepage 等仍
  可结构化保留），复杂/多行值降级为字符串文本；该回退保证 `name` 至少存活，绝不因格式
  不规则丢弃整篇定义。

CodeBuddy 子代理 frontmatter（官方文档已证实），导入后必须结构化保留：

```yaml
name、description
model、effort
maxTurns
tools、disallowedTools
skills
memory（project/agent 等）
background
isolation（worktree 等）
permissionMode（注意：插件级 Agent 的 permissionMode/mcpServers 官方明确忽略）
```

插件 Agent 的 `mcpServers`/`permissionMode` 字段，来源运行时本身忽略（官方文档明确、出于安全原因），导入器不得把它当作 Agent 级 Connector 授权，应记录为 `ignored-by-source-runtime`。

## 6. Team 扩展导入（WorkBuddy/CodeBuddy 特有）

`teamInfo`（leadAgent/memberAgents/expertType=team）不属于通用插件规范字段，按以下步骤：

```text
1. 把 lead 与每个 member 的 agents/*.md 先标准化为 AgentDefinition；
2. 生成 AgentTeamDefinition：lead_agent_id + member_agent_ids；
3. 成员文件缺失/不可解析 → 该 Team 标记 manual-review；
4. agents/ 目录本身 ≠ Team：没有 teamInfo 的插件只导入 AgentDefinition。
```

示例（软件工程团队）：

```text
Team: software-company
Lead:  software-team-lead
Members: software-product-manager, software-architect,
         software-engineer, software-qa-engineer
```

→ 1 个 AgentTeamDefinition + 5 个 AgentDefinition。AgentExecutionTemplate 和 Planning Context 都只在 TeamRun 创建时由 Runtime Adapter 生成；Importer 不拼接团队 Prompt。V1 TeamRun 支持 Planning Context 驱动的 planned DAG、局部并行、事件、重试和 replan；不要求完整 Mailbox。

## 7. 路径变量映射

| CodeBuddy 变量 | Agent Store 变量 |
|---|---|
| `${CODEBUDDY_PLUGIN_ROOT}` | `${AGENT_STORE_PLUGIN_ROOT}`（当前版本目录） |
| `${CODEBUDDY_PLUGIN_DATA}` | `${AGENT_STORE_PLUGIN_DATA}`（跨版本持久数据） |
| `${CODEBUDDY_PROJECT_DIR}` | `${AGENT_STORE_WORKSPACE}` |
| `${user_config.KEY}` | CredentialSchema 引用 + 运行时注入 |

规则：

- 插件只能引用快照内路径；插件目录外的相对引用（如 `../shared-utils`）安装后不保证工作，导入时标记警告；
- 同一市场内的特定符号链接可解析复制；外部符号链接一律拒绝。

## 8. 市场与依赖

- 市场来源支持：本地目录 / GitHub 仓库 / Git URL / HTTP marketplace.json（官方文档已证实）；
- **实现状态（2026-08 迭代 Phase 2 阶段 B）**：`directory` + `github` + `git` + `url`
  全部落地（`plugin_marketplaces` 注册表 + `market_fetch` 获取层：git2 克隆 /
  HTTP 条件下载 → staging 校验 → 原子晋升 → last-good；`market/refresh` 按
  resolved_revision/ETag 新鲜度短路）+ `agent/run` 结构化 `mentions`
  （agent→已安装 preset、skill→`included_skills`、connector→`mcp_server_ids`，
  TC-INS-007）；agent-store preset `agent-store: <name>` 命名进入兼容白名单；
  `plugin.json` 展示元数据完整保真（displayName/profession/displayDescription/
  tags/quickPrompts/defaultInitPrompt/expertType/categoryId + avatars 资产经
  受控资产端点 serve，TC-IMP-015）；
  **Store（winget 式商店）**：`store/list` 聚合所有启用市场条目（含未导入的，
  四类 kind + plugin.json 展示保真 + 安装状态/可更新标记）、
  `store install-entry` 一键安装（导入+注册幂等）、store 资产端点；
  市场探测新增 `.codebuddy-plugin/marketplace.json`（`plugins[]`，WorkBuddy
  专家市场布局，TC-IMP-016）；
  URL 市场条目外源标记 `external`（文档语义：
  只镜像清单内联条目）；
- 市场条目字段（name、source、version、strict、commands、agents、skills、hooks、mcpServers）可与插件 Manifest 合并，检查冲突；
- `strict=true`：要求插件源自带 plugin.json；`strict=false`：市场条目可补充/代替清单；
- 依赖可用字符串或对象（name + version + marketplace），版本用 SemVer 范围；
- 安装作用域：user / project / local / managed（保留到 Agent Store：user / workspace / local / managed）；
- 自动更新：官方市场默认开启、第三方默认关闭；Agent Store 记录 source、resolved_version、installed_version、update_policy、last_checked_at，V1 不自动更新第三方来源。

## 9. ID 与溯源规则

- 来源 slug 不作为业务 ID；建议 `wb-<pluginId>-<agentId>`（来源稳定前提下）；
- 快照记录：source_kind、source_uri、relative_path、declared_version、resolved_revision、content_digest、imported_at；
- 相同 digest 重复导入 → 返回已有快照（幂等）；
- 来源路径仅在内部追溯，不出现在对外 API/协议。

## 10. 安全与执行边界（导入期）

- 导入只复制与解析，不执行任何脚本/命令；
- 生命周期钩子、LSP、bin/scripts 在对应执行安全模型落地前不启用；
- 权限声明与风险标签不是沙箱；不受信内容以「来源不可信」对待；
- 敏感字段（API Key、Token 等）导入时只建立 schema 与引用，值由用户后续通过安全存储提供；文档统一写作 `[REDACTED]`。
- **MCP 连接器的 `env`（`mcpServers` → ConnectorDefinition）同口径**：**敏感键**的值只留引用——改写为 `secret:<KEY>`（`17` §6 / `21` D5=C），真值由 `~/.agent-store/config.toml` 的 `[credentials]`（或进程 env）提供，MCP 启动时按引用解析注入；值既不进 Snapshot 也不进 DB 行，缺失时该变量被省略（2026-09-11 收口，`16` R22 / `17` §10 P1）。

## 11. 导入结果契约

每次导入必须返回结构化结果，不以“目录复制完成”作为成功标准：

```json
{
  "snapshot": {},
  "definitions": [],
  "compatibility_report": {},
  "warnings": [],
  "errors": [],
  "status": "completed",
  "component_status": {
    "semantic_status": "compatible",
    "runtime_status": "not-verified",
    "distribution_status": "local-only"
  }
}
```

`status` 取值：

```text
completed
completed-with-warnings
blocked
failed
```

`errors` 非空时：若只影响可选组件，使用 `completed-with-warnings`；若影响必要定义、强依赖或快照完整性，使用 `blocked`；Importer 自身异常且无法生成可信结果时使用 `failed`。Snapshot 可以查询，但只有 `runtime_status=runtime-verified` 且 `distribution_status` 允许时才能启用或分发。

### 11.1 阻断规则

以下情况阻断整个 Snapshot 安装：

- Manifest 无法解析或缺少必需身份字段；
- 路径遍历、外部符号链接逃逸或快照复制失败；
- 内容 digest 计算失败；
- 必需依赖无法解析，且来源声明为强依赖；
- 同一来源身份与版本存在 digest 冲突；
- **市场条目声明 `strict=true`，但插件来源未自带 `.codebuddy-plugin/plugin.json`**（§8；`17` §7）。条目仍留在目录页并给出原因，但**不可安装**——静默消失会让这条规则无人可见（2026-09-11 实现，`17` §10 P4）。

> 上一条的「来源」= 该条目 `source` 解析出的目录。`strict=false`（默认）**不适用**本条：那种情况下市场条目可以补充或代替清单，属 `17` §10 P5（未实现）。

> **「且来源声明为强依赖」怎么判定（2026-09-11 落地时写明）**：V1 的依赖声明形态只有 `{name, version?, marketplace?}`（§8），**没有 `required` 之类的标记**，故以**是否声明了 `version` 范围**为准——声明了即视为来源提出了要求（可阻断），只写名字则只是备注（仅登记、永不阻断）。判定在**导入期**做，逃生口＝`[import].strict_dependencies`（**默认关**，完全保留历史行为）；解析来源与两处已知边界见 `17` §10 P3 / `16` R23 落地记录。

### 11.2 部分失败规则

以下情况允许生成 Snapshot，但对应组件必须标记状态并写入 warnings/errors：

- 单个 Agent/Skill/Command 文件无法解析；
- Skill 缺少可选市场元数据；
- Team 成员缺失或字段不完整；
- MCP 缺少用户凭据；
- Hook、LSP、脚本只完成静态导入；
- 来源字段被来源运行时忽略。

部分失败不得产生“可运行”状态。定义只有在必要字段完整、依赖满足、目标 Runtime 适配通过后，才能标记 `compatible` 或 `compatible-with-adapter`。

### 11.3 安装状态机（roadmap Phase 2）

导入只登记不可变快照（`installed=0`）；安装把组件注册进运行时并记录状态：

```text
not-installed ── install ──▶ installed ── disable ──▶ disabled
     ▲                          │  ▲                    │
     └────── uninstall ─────────┘  └──── enable ────────┘
```

- `installed` / `disabled` 字段存放在 `plugin_snapshot_components`（迁移 054）；
- `runtime_ref`（JSON：`{type, location, mcp_server_id}`）与 `preset_id`（agent/team）记录运行时目标，仅供内部追溯；
- `uninstall` 移除运行时产物并清除状态（快照行保留）；`disable` 保留产物只翻转状态位；
- 安装幂等：重复 `install` 覆盖相同的运行时注册，不产生重复行。

## 12. 输出物（Importer 交付验收）

1. PluginSnapshot（JSON 或 DB 记录）：字段见 `01-domain-model.md` §9；
2. 标准化定义：Agent/Team/Skill/Connector/Command/Hook/LSP/CredentialSchema；
3. CompatibilityReport：每个组件一项 `semantic_status`、`runtime_status`、`distribution_status` + 原因；`unsupported-auth`、`ignored-by-source-runtime` 作为原因码，不作为新的一级状态；
4. 导入日志（警告清单：路径越界、变量未解析、字段被来源运行时忽略、依赖缺失）。

验收案例：导入 `software-company` 插件后必须可见 5 个 Agent、1 个 Team、TeamInfo 解析成功、无未标记丢弃项。
---

## 13. 验收用例（TC-IMP / TC-INS）

> 本节由 `agent-store-v1-test-cases.md` 原 §3 并入（2026-09-11 文档合并）；TC 编号与用例正文保持不变，内部分节号沿用原文。

### 3. Importer 与 Catalog

#### TC-IMP-001：导入有效 Plugin

- 等级：P0
- 前置：有效 Plugin manifest 和受控源目录
- 操作：执行 Importer
- 断言：生成不可变 PluginSnapshot、digest、来源和版本；状态为 `completed` 或 `completed-with-warnings`
- 证据：snapshot_id、digest、definitions 数量、CompatibilityReport

#### TC-IMP-002：导入 software-company

- 等级：P0
- 操作：导入包含 Team 扩展信息的插件
- 断言：生成 5 个 AgentDefinition、1 个 AgentTeamDefinition；Lead 和 member IDs 正确；V1 可生成固定 Participant 配置
- 证据：定义列表、成员关系、来源路径摘要、digest

#### TC-IMP-003：agents 目录不自动生成 Team

- 等级：P0
- 操作：导入只有 `agents/`、没有明确 Team 配置的插件
- 断言：只生成 AgentDefinition[]，不生成 AgentTeamDefinition

#### TC-IMP-004：路径遍历阻断

- 等级：P0
- 操作：提供 `../`、绝对路径和快照外引用
- 断言：导入状态为 `blocked`；不产生可运行定义；写入安全错误

#### TC-IMP-005：符号链接逃逸阻断

- 等级：P0
- 操作：提供指向快照目录外的符号链接
- 断言：拒绝安装或复制；无外部文件进入 Snapshot

#### TC-IMP-006：digest 冲突阻断

- 等级：P0
- 操作：同一来源身份/版本提交不同内容 digest
- 断言：状态为 `blocked` 或 `failed`；不得覆盖既有不可变 Snapshot

#### TC-IMP-007：部分组件失败

- 等级：P1
- 操作：让单个 Skill 或 Command 文件无法解析
- 断言：其他合法组件可导入；结果为 `completed-with-warnings`；失败组件有 CompatibilityReport 条目

#### TC-IMP-008：高风险组件静态导入

- 等级：P0
- 操作：导入 Hook、bin、scripts、LSP
- 断言：保存来源元数据和兼容性状态；导入过程不执行任意进程或脚本

#### TC-IMP-009：凭据只生成 Schema

- 等级：P0
- 操作：导入含 userConfig/token schema 的插件
- 断言：只生成 CredentialSchema/Binding 引用；真实值不进入 Snapshot、日志和公共响应

#### TC-IMP-010：文件路径声明 + 对象形式依赖

- 等级：P1
- 操作：导入 manifest 以文件路径声明组件（`./agents/lead.md`）且 `dependencies` 为对象（`{"connectors":["x"]}`）的插件
- 断言：文件被逐个导入为组件；对象依赖归一化为条目（带 `group` 标记）；不因形状差异发生 blocked

#### TC-IMP-011：CLI 连接器目录

- 等级：P1
- 操作：导入 `connectors/<id>/`（`cli.json` + `skills/*/SKILL.md`）
- 断言：生成 1 个 `connector` 组件（kind=cli，含 runtime/auth 摘要）与随附 `skills/` 全部 SkillDefinition；`cli.json` 缺名下以目录名为身份

#### TC-IMP-012：市场格式兼容（CRLF / 宽松 YAML）

- 等级：P1
- 操作：导入含 CRLF 行尾 SKILL.md、以及含未加引号引号/内嵌 JSON 的非严格 frontmatter 的市场
- 断言：CRLF 正常解析；非严格 YAML 回退到宽松行级解析（name/description 至少保留），不整篇丢弃

#### TC-IMP-013：author 对象 + 字符串组件根

- 等级：P1
- 操作：导入 `author: {"name": "..."}` 且 `agents: "./agents/x.md"`（字符串而非数组）的插件
- 断言：author 归一化为 `name <email>`；字符串组件根等价于单元素数组；同名 Agent+Skill 时 Skill 加 `-skill` 后缀消歧

#### TC-IMP-014：单 Skill 目录

- 等级：P1
- 操作：以 `workbuddy-skill-market` 导入 `skills/<slug>/`（无 marketplace.json，根含 SKILL.md）
- 断言：单技能可导入；身份 = 目录名；不因缺 marketplace.json 被 blocked

#### TC-IMP-015：展示元数据保真（plugin.json 完整镜像）

- 等级：P1
- 操作：导入真实专家结构（`plugin.json` 携带 `displayName`/`profession`/
  `displayDescription`/`tags`/`quickPrompts`/`defaultInitPrompt`/`avatar`/
  `expertType`/`categoryId` + agent frontmatter 同名字段 + `avatars/*.png`）
- 断言：
  - 全部展示字段进入 agent payload（plugin.json 优先于 frontmatter）；
  - `quickPrompts`/`tags` 双语数组完整保留（=WorkBuddy 专家卡「专家帮你做」）；
  - 头像资产文件随快照拷贝（`avatars/expert.png` 在快照内）；
  - `agent/get` 暴露 `display_name`/`profession`/`avatar_url`/`quick_prompts`；
  - 资产端点：`GET /imports/{snapshot}/assets/avatars/expert.png` 返回
    `image/png`；路径穿越/非白名单类型/非法 snapshot id → 4xx；prompt
    文件（`SKILL.md`/`*.md`）永不被 serve

#### TC-IMP-016：插件市场探测（`.codebuddy-plugin/marketplace.json`）

- 等级：P1
- 操作：以 `directory` 源添加真实 WorkBuddy 专家市场布局
  （`market/.codebuddy-plugin/marketplace.json` 含 `plugins[]` +
  `market/plugins/<id>/.codebuddy-plugin/plugin.json`）
- 断言：探测到 1 个 plugin-market 市场；`market/get` 条目数 = `plugins[]` 数
  （每个条目 source = `plugins/<id>`，描述来自清单）；缺 plugin.json 的
  行被跳过；市场名取自清单 `name`

#### TC-IMP-017：Store 聚合与一键安装

- 等级：P1
- 操作：`market/add`（专家市场）后 `store/list` →
  `store/{market}/entries/{entry}/install` → 再 `store/list`
- 断言：
  - `store/list` 返回全部条目（含未导入的）；kind 正确（agent/team/skill/
    connector）；`name`/`display_name`/`profession`/`quick_prompts`/`avatar_url`
    保真；`installed=false`、`update_available=false`、`snapshot_id` 空；
  - `store install-entry` 返回 `snapshot_id`/`version`/`installed_count>=1`，
    `reused=false`；
  - 再次 `store/list`：该条目 `installed=true`、`snapshot_id` 非空；
  - 再次 `store install-entry`：`reused=true`（幂等）；
  - store 资产端点：`GET /store/{market}/entries/{entry}/assets/{path}` 白名单
    类型 200；穿越/非白名单 → 4xx

#### TC-INS-001：安装注册到运行时

- 等级：P1
- 操作：导入 software-company 后执行 `install/run`
- 断言：`installed_count > 0`；skill 物化到 `{skills}/agent-store/{snapshot_id}/{slug}/`；
  agent/team 创建 Preset；connector upsert 进 `mcp_servers`；组件状态 `installed`

#### TC-INS-002：禁用 / 启用 / 卸载状态机

- 等级：P1
- 操作：对已安装组件依次 `disable` → `enable` → `uninstall`
- 断言：状态 `installed → disabled → installed → not-installed`；卸载后快照与组件行保留

#### TC-INS-003：市场添加与条目发现

- 等级：P1
- 操作：`market/add`（directory 源，含 `.codebuddy-skill/marketplace.json` + `skills/`）
- 断言：注册表行出现；`market/list` 返回 1 项；`market/get` 条目数 = 清单条目数；
  同源重复添加幂等（不产生新行）
- 注：非 directory 源返回 `bad_request`；不含任何清单/插件子目录的路径返回 `not_found`

#### TC-INS-004：条目导入 + 级联卸载

- 等级：P1
- 操作：`market/get` → `entries/{entry}/import` → `install/run` → `market/remove`
  （cascade=true）
- 断言：条目导入复用导入管线并记录 provenance（不出现于公共响应）；安装成功；
  remove 后已安装组件状态回到 `not-installed`（快照行保留于 history）；市场从
  `market/list` 消失；二次 remove 返回 `not_found`

#### TC-INS-005：Git 源获取与刷新（阶段 B）

- 等级：P1
- 操作：以 `git` 源添加市场（本地 bare repo 作 origin）→ `market/get` →
  `market/refresh` 两次
- 断言：add 同步克隆并渲染条目（相对路径解析完整树）；`refresh` 同 commit →
  `changed=false`（no-op）；live 根存在于工作区市场目录；条目导入后快照携带
  `resolved_revision` provenance

#### TC-INS-006：HTTP 源清单校验与外部条目（阶段 B）

- 等级：P1
- 操作：以 `url` 源添加市场（本地 mock server 返回 marketplace.json）→ refresh
- 断言：清单无 `name`/无条目数组 → add 返回错误；合法清单 → 条目可发现；
  声明外部源（GitHub/NPM）条目标记 `source_kind=external`，导入返回 `bad_request`；
  `refresh` 带 `If-None-Match`，`304`/相同 ETag → `changed=false`

#### TC-INS-007：@Mention 解析与 run 注入（阶段 B 扩展）

- 等级：P1
- 操作：Composer `@` 菜单选中 agent/skill/connector → `agent/run` 携带
  `mentions`（agent/skill/connector 三类）→ 观察
- 断言：
  - agent mention 解析为已安装 preset（未安装 → `agent_not_installed`；
    多个 agent → `invalid_mentions`；与显式 `agent_id` 冲突 → `invalid_mentions`）；
  - skill mention 冻结进 `included_skills`（run 上下文随 agent 挂载）；
  - connector mention 追加 `mcp_server_ids`（未启用 → `connector_unavailable`）；
  - `mentions` 缺省时行为与旧 `agent/run` 一致（向后兼容）；
  - 任意用户 preset（`agent-store:` 前缀外）不能通过 `agent/run` 启动


---

## 14. 附录 · Importer 运行时验证证据

> 本节由 `importer-runtime-evidence.zh.md` 整体并入（2026-09-11）。原文的时间戳与「历史实测快照，非契约」定性**保持不变**。


> 日期：2026-08-26
> 目标：`docs/agent-store/02-codebuddy-workbuddy-import-spec.md`（Importer 落地）与
> `03-...-compatibility-matrix.md`（三态兼容推导），对齐本文 §13 的 TC-IMP-001~009
> 状态：✅ 全部通过（`software-company` fixture：5 个 AgentDefinition + 1 个 AgentTeamDefinition）
> 扩展：TC-IMP-010/011（文件路径声明+对象依赖；CLI 连接器目录）

### 1. 实现范围

| 组件 | 位置 | 内容 |
|---|---|---|
| 导入管线 | `crates/backend/nomifun-importer/`（新 crate） | manifest/frontmatter 解析、受控复制+sha256 树摘要、组件标准化、幂等与 digest 冲突、三态兼容推导 |
| 快照目录层 | `crates/backend/nomifun-db`（migration 053） | `plugin_snapshots` + `plugin_snapshot_components`（v3 契约注册：PRODUCT_TABLES / UUIDv7 CHECK / LOGICAL_REFERENCES） |
| 协议层 | `nomifun-api-types::app_server`、`nomifun-app-server` | `import/*`（HTTP）、`agent/list` `agent/get` `team/list` `team/get`（WS，05 §4.1/4.2）；`imports`/`agents`/`teams` capabilities |
| 组成根 | `nomifun-app/src/app_server_importer.rs` + `routes.rs` | 注入 ImporterService + SQLite 仓储；缓存根 `{work_dir}/agent-store-imports/` |
| WebUI | `web/`（CatalogView + protocol + agents/teams/imports 客户端） | 导入表单/结果三态报告/历史/组件详情；Agent/Team 目录 tab |

### 2. 验收结果（TC-IMP-001~009）

`crates/backend/nomifun-importer/tests/importer_tests.rs`，fixture 在 `tests/fixtures/`：

| 用例 | 断言要点 | 结果 |
|---|---|---|
| TC-IMP-001 | 合法 plugin → 不可变 PluginSnapshot（digest/来源/版本），状态 `completed` | ✅ |
| TC-IMP-002 | `software-company` → 5 AgentDefinition + 1 Team；lead/member 指向已导入 agent id；frontmatter 字段结构化保留 | ✅ |
| TC-IMP-003 | 只有 `agents/` 无 teamInfo → 不生成 Team | ✅ |
| TC-IMP-004 | `../`、绝对路径 → `blocked`，无入库/无物化目录，错误不含绝对来源路径 | ✅ |
| TC-IMP-005 | 符号链接逃逸（unix）→ `blocked`，外部文件永不进入快照 | ✅ |
| TC-IMP-006 | 相同 digest 重复导入 → `reused=true` 复用同一 snapshot；同身份不同 digest → `blocked` 不覆盖 | ✅ |
| TC-IMP-007 | 单组件坏 frontmatter → `completed-with-warnings`，其余组件照常导入 | ✅ |
| TC-IMP-008 | hooks/bin/scripts/lsp → 静态导入（`manual_review`/`static_only`），不执行 | ✅ |
| TC-IMP-009 | userConfig 敏感字段 → 只生成 CredentialSchema；`[REDACTED]` 警告；值不进入 DB/日志/公共响应 | ✅ |
| TC-IMP-010 | 组件以文件路径声明（`./agents/lead.md`）+ `dependencies` 对象形式 → 每个文件导入为组件；对象依赖归一化（带 `group`） | ✅ |
| TC-IMP-011 | CLI 连接器目录（`cli.json` + `skills/*/SKILL.md`）→ 1 connector（kind=cli）+ 全部随附 SkillDefinition | ✅ |
| TC-IMP-012 | CRLF 行尾 frontmatter + 非严格 YAML → CRLF 正常解析；非严格回退到宽松行级解析（name/description 存活） | ✅ |
| TC-IMP-013 | `author` 对象 + 字符串组件根（`agents: "./agents/x.md"`）→ 归一化导入；同名 Agent+Skill 自动 `-skill` 消歧 | ✅ |
| TC-IMP-014 | 单 Skill 目录（无 marketplace.json，根含 SKILL.md）→ 身份=目录名，可导入 | ✅ |

另含市场来源用例：`workbuddy-skill-market` → 2 个 Skill（保留 `$ARGUMENTS`）；`workbuddy-connector-market` → 2 个 Connector；缺失清单 → `blocked`；来源目录不存在 → `ImportError::SourceNotFound`（HTTP 404 `import_source_not_found`）。

真实市场目录验证（本机 `~/.workbuddy/`）：
- `connectors-marketplace` 根（`workbuddy-connector-market`、193 个索引条目采样）→ `completed`，153/153 组件（中文 display name 用 ASCII `id` 生成组件 id）；
- `skills-marketplace` 根（`workbuddy-skill-market`）→ `completed`，262/262 技能组件（含 CRLF 与宽松 YAML 文件）；
- `plugins/marketplaces/experts|codebuddy-plugins-official|cb_teams_marketplace` 全部插件目录 → 0 blocked（`author` 对象、字符串组件根、同名 Agent+Skill 均兼容）；
- 单 skill 目录（`skills/qcc-company` 等）→ 全部 `completed`。

### 3. 自动化证据

```text
cargo test -p nomifun-importer（lib 16 项 + 集成 15 项）            ✅ 全过
cargo test -p nomifun-app-server                                     ✅ 50 passed（含 agent/team/imports WS 与 HTTP impl 契约测试）
cargo test -p nomifun-db --lib                                        ✅ 436 passed（含新增 plugin_snapshot 仓储 5 项）
cargo test -p nomifun-app --test importer_e2e                         ✅ 真 app HTTP 全链路（init → import → history → detail → 幂等复用 → 404）
cargo fmt --check（5 crate）                                          ✅ 无差异
cargo clippy -p nomifun-importer --all-targets                        ✅ 本方案文件零警告
bun run typecheck（web/）                                             ✅ 通过
```

### 4. 关键安全边界（已验证）

- **零执行**：导入只读取与复制；`walk.rs` 拒绝一切符号链接；socket/fifo/设备文件视为敌意内容整体阻断；
- **凭据**：userConfig 只生成字段 schema（`sensitive` 标记）；`default`/`value` 永不入库；错误与警告统一 `[REDACTED]`；materialized 目录是来源字节的不变镜像（供来源追溯），标准化定义与目录记录不含凭据值；
- **路径**：清单路径必须 `./` 开头、拒绝 `..` 与绝对路径（`validate_relative_path`）；绝对来源路径只存 `source_uri`（内部追溯），不出现在公共协议（02 §9）。

### 5. 已知边界（如实披露）

- 市场来源仅本地目录；GitHub/Git/HTTP 市场源为 `compatible-with-adapter`，未实现；
- 导入后**运行时激活**（Skill 加载、MCP 配置注入、Preset 绑定、TeamRun）属后续 Gate；`runtime_status` 一律 `not-verified`，UI 与协议如实显示；
- 同插件不同版本导入会产生同 `component_id` 的目录条目（`agent/list` 按最新快照优先去重展示；`plugin_snapshot_components.component_id` 未注册为全局 UUIDv7 业务列，v3 契约按本地唯一处理）；
- 插件级 Agent 的 `mcpServers`/`permissionMode` 记录 `ignored-by-source-runtime` 原因码，不映射为授权（02 §5.1）。
