# CodeBuddy / WorkBuddy 导入规范

> 状态：架构冻结（Phase 0）；Importer 待实现验证；发布阻断
> 日期：2026-08-26
> 前置：`00-architecture-decision.md`、`01-domain-model.md`
> 依据：https://www.codebuddy.cn/docs/cli/plugins、/plugins-reference、/plugin-marketplaces、/sub-agents、/agent-teams（官方文档已提取正文部分）

## 1. 导入范围

| 来源 | 位置 | 对象 |
|---|---|---|
| CodeBuddy/WorkBuddy 插件 | `.codebuddy-plugin/plugin.json` + 组件目录 | PluginSnapshot |
| WorkBuddy Skill 市场 | `.codebuddy-skill/marketplace.json` + `skills/<slug>/` | SkillDefinition |
| WorkBuddy Connector 市场 | `.codebuddy-connector/connectors.json` + `connectors/<slug>/` | ConnectorDefinition + CredentialSchema |

约束：导入器只把来源当作输入格式，不要求 allo 原生解析器理解来源字段；标准输出为不可变 PluginSnapshot 与标准化定义。

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
| `version` / `description` / `author` | 元数据；version 参与快照版本 |
| `agents` | 目录引用（相对、`./` 开头）→ AgentDefinition |
| `skills` | 目录引用 → SkillDefinition |
| `commands` | 目录引用 → CommandDefinition |
| `hooks` | 文件/目录引用 → LifecycleHookDefinition |
| `mcpServers` / `.mcp.json` | 插件级 MCP 配置 → ConnectorDefinition |
| `lspServers` / `.lsp.json` | → LspDefinition（V1 元数据级） |
| `userConfig` | 配置 schema → CredentialSchema；敏感项进安全存储 |
| `dependencies` | 插件依赖（可含版本/市场约束）→ PluginDependency |
| `defaultEnabled` / `channels` | 安装/启用策略元数据 |

规则：

- Manifest 路径一律相对于插件根，必须以 `./` 开头；自定义组件路径替换默认目录，除非默认目录也列入数组；
- `name` 同时是 Skill 命名空间（如 `my-plugin:hello`），导入后保留该命名空间语义（`plugin:skill` 形态）。

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
- 同一来源身份与版本存在 digest 冲突。

### 11.2 部分失败规则

以下情况允许生成 Snapshot，但对应组件必须标记状态并写入 warnings/errors：

- 单个 Agent/Skill/Command 文件无法解析；
- Skill 缺少可选市场元数据；
- Team 成员缺失或字段不完整；
- MCP 缺少用户凭据；
- Hook、LSP、脚本只完成静态导入；
- 来源字段被来源运行时忽略。

部分失败不得产生“可运行”状态。定义只有在必要字段完整、依赖满足、目标 Runtime 适配通过后，才能标记 `compatible` 或 `compatible-with-adapter`。

## 12. 输出物（Importer 交付验收）

1. PluginSnapshot（JSON 或 DB 记录）：字段见 `01-domain-model.md` §9；
2. 标准化定义：Agent/Team/Skill/Connector/Command/Hook/LSP/CredentialSchema；
3. CompatibilityReport：每个组件一项 `semantic_status`、`runtime_status`、`distribution_status` + 原因；`unsupported-auth`、`ignored-by-source-runtime` 作为原因码，不作为新的一级状态；
4. 导入日志（警告清单：路径越界、变量未解析、字段被来源运行时忽略、依赖缺失）。

验收案例：导入 `software-company` 插件后必须可见 5 个 Agent、1 个 Team、TeamInfo 解析成功、无未标记丢弃项。