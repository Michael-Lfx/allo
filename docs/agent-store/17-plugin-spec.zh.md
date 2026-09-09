# 插件规范（兼容层）

> 状态：规范 v1（2026-09-09）。
> **定位：本规范只定义「兼容层」。** 依 `16` §4 Q5 的决策——**先只做兼容层**——Agent Store **尚未定义原生插件格式**；此处规定的是「Agent Store 当前接受什么、如何归一化、边界在哪」，而非插件作者应当遵循的自有格式。
> 字段级映射详表见 `02-codebuddy-workbuddy-import-spec.md`（本规范不重复，只固定必填/可选/默认与冲突规则）。
> 代码事实来源：`crates/backend/nomifun-importer/`、`crates/backend/nomifun-extension/`、`crates/backend/nomifun-api-types/src/app_server.rs`。

---

## 1. 定位与非目标

**当前接受的格式**是 CodeBuddy / WorkBuddy 生态的既有格式（`.codebuddy-plugin/`、`.codebuddy-skill/`、`.codebuddy-connector/`）。Agent Store 的角色是**归一化 + 快照**，不是定义格式。

因此：

- 字段语义、默认值、保留位**以兼容源为准**；Agent Store 不擅自扩展语义；
- 兼容源新增字段时，Agent Store 的行为是「保真透传展示元数据」或「忽略」，**不猜测**；
- 一旦未来定义原生格式，本规范升级为 v2，兼容层降为「导入源」并保留。

**非目标（V1）**：原生格式定义；插件脚本执行；生命周期钩子执行；LSP；`bin/` / `scripts/` 运行；权限沙箱（见 §6）。

---

## 2. 插件包布局（兼容层接受）

| 路径 | 含义 | 必需 |
| --- | --- | --- |
| `.codebuddy-plugin/plugin.json` | 插件清单（根即一个插件） | 插件型必需 |
| `.codebuddy-plugin/marketplace.json` | 专家市场清单（`plugins[]`，条目在 `plugins/<id>/`） | 市场型必需 |
| `.codebuddy-skill/marketplace.json` | 技能市场清单（条目在 `skills/<slug>/`） | 技能市场必需 |
| `skills/<slug>/SKILL.md` | 单技能目录（无清单） | 单技能必需 |
| `.codebuddy-connector/connectors.json` | 连接器市场清单（条目在 `connectors/`） | 连接器市场必需 |
| `cli.json` | 单 CLI 连接器 | 单 CLI 必需 |
| `agents/*.md` | Agent 定义（frontmatter + 正文） | 可选 |
| `hooks/`、`commands/` | 兼容源保留位 | 可选，**不启用** |

---

## 3. 清单字段与冲突规则

三份清单按 §2 的路径发现。字段分组：

| 组 | 字段（兼容层接受） | 处理 |
| --- | --- | --- |
| 标识 | `name`、`version` | `name` 参与 ID 派生（§5）；`version` 记为 `declared_version` |
| 展示元数据 | `displayName`、`profession`、`displayDescription`、`tags`、`quickPrompts`、`defaultInitPrompt`、`expertType`、`categoryId` | **保真透传**到展示层；本地化字段按语言取用 |
| 组件声明 | `agents`、`skills`、`hooks`、`commands`、`mcpServers` | `agents` / `skills` / `mcpServers` 参与归一化；`hooks` / `commands` 仅记录 |
| 依赖 | `dependencies` | 见 §7 |
| 严格性 | `strict` | `true`：要求插件源自带 `plugin.json`；`false`：市场条目可补充或代替清单 |
| 资产 | `avatars` 等相对路径 | 经受控资产端点 serve；**不落绝对路径** |

**冲突规则**：市场条目字段与插件清单字段合并时逐项比对，冲突以**插件清单为准**并记录冲突（不静默覆盖）。

---

## 4. 组件映射

| 来源 | 归一化为 | 备注 |
| --- | --- | --- |
| `agents/*.md` | `AgentDefinition` | frontmatter 字段按 `02` §5.1 保留清单 |
| `skills/<slug>/` | `SkillDefinition` | 含 `SKILL.md` 与其附属文件 |
| 连接器清单条目 | `ConnectorDefinition` | 凭据按 §6 处理 |
| Team 扩展（WorkBuddy/CodeBuddy 特有） | `AgentTeamDefinition` | 见 `02` §6 |
| 插件根 | `PluginSnapshot` | 一次导入产出一个不可变快照 |

---

## 5. 版本、兼容性与溯源

| 项 | 语义 |
| --- | --- |
| `declared_version` | 清单声明的版本（可缺省） |
| `resolved_revision` | 解析到的修订（git commit / HTTP ETag / 内容摘要标记） |
| `content_digest` | 快照内容摘要，用于幂等与去重 |
| `imported_at` | 导入时间戳 |
| ID | 来源 slug 不作为业务 ID；建议 `wb-<pluginId>-<agentId>`（来源稳定前提下） |

**幂等**：相同 `content_digest` 重复导入 → 返回已有快照，不产生新快照。

**兼容性状态**取值：`compatible` / `compatible-with-adapter` / `manual-review` / `pending-legal-review`（未确认版权资源不进公开分发）。

**对外不暴露来源路径**：`source_kind` / `source_uri` / `relative_path` 仅供内部追溯。

---

## 6. 权限、凭据与安全边界（导入期）

- 导入**只复制与解析，不执行任何脚本或命令**；
- 生命周期钩子、LSP、`bin/` / `scripts/` 在执行安全模型落地前**不启用**；
- 权限声明与风险标签**不是沙箱**；不受信内容一律以「来源不可信」对待；
- 敏感字段（API Key、Token 等）导入时**只建立 schema 与引用**，值由用户后续通过安全存储提供；文档与日志统一写作 `[REDACTED]`。

---

## 7. 依赖与冲突

- `dependencies` 支持**字符串**或**对象**（`name` + `version` + 可选 `marketplace`）；
- 版本使用 **SemVer 范围**；
- 依赖不可满足时：阻断安装并给出缺失项，不做静默降级；
- `strict=true` 且插件源缺 `plugin.json` → 阻断（见 `02` §11.1 阻断规则）。

---

## 8. 演进

- 原生插件格式**待生态起量后**再定义；届时本规范升级 v2，并明确原生格式与兼容层的字段优先级、迁移路径与弃用窗口；
- 在原生格式定义前，**不接受**任何以 Agent Store 名义扩展的私有字段。

---

## 9. 验收口径

按本规范 + `02`，能够**独立复现**一个可被 `market/add` → `market/refresh` → `store/list` → 安装 的插件包，且现有真实市场（experts / skills / connectors）逐条对照无例外。字段级细节以 `02` 为唯一正文。
