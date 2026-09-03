# Agent Store Importer（Phase 1）— 验证证据

> 日期：2026-08-26
> 目标：`docs/agent-store/02-codebuddy-workbuddy-import-spec.md`（Importer 落地）与
> `03-...-compatibility-matrix.md`（三态兼容推导），对齐 `agent-store-v1-test-cases.md` TC-IMP-001~009
> 状态：✅ 全部通过（`software-company` fixture：5 个 AgentDefinition + 1 个 AgentTeamDefinition）
> 扩展：TC-IMP-010/011（文件路径声明+对象依赖；CLI 连接器目录）

## 1. 实现范围

| 组件 | 位置 | 内容 |
|---|---|---|
| 导入管线 | `crates/backend/nomifun-importer/`（新 crate） | manifest/frontmatter 解析、受控复制+sha256 树摘要、组件标准化、幂等与 digest 冲突、三态兼容推导 |
| 快照目录层 | `crates/backend/nomifun-db`（migration 053） | `plugin_snapshots` + `plugin_snapshot_components`（v3 契约注册：PRODUCT_TABLES / UUIDv7 CHECK / LOGICAL_REFERENCES） |
| 协议层 | `nomifun-api-types::app_server`、`nomifun-app-server` | `import/*`（HTTP）、`agent/list` `agent/get` `team/list` `team/get`（WS，05 §4.1/4.2）；`imports`/`agents`/`teams` capabilities |
| 组成根 | `nomifun-app/src/app_server_importer.rs` + `routes.rs` | 注入 ImporterService + SQLite 仓储；缓存根 `{work_dir}/agent-store-imports/` |
| WebUI | `web/`（CatalogView + protocol + agents/teams/imports 客户端） | 导入表单/结果三态报告/历史/组件详情；Agent/Team 目录 tab |

## 2. 验收结果（TC-IMP-001~009）

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

## 3. 自动化证据

```text
cargo test -p nomifun-importer（lib 16 项 + 集成 15 项）            ✅ 全过
cargo test -p nomifun-app-server                                     ✅ 50 passed（含 agent/team/imports WS 与 HTTP impl 契约测试）
cargo test -p nomifun-db --lib                                        ✅ 436 passed（含新增 plugin_snapshot 仓储 5 项）
cargo test -p nomifun-app --test importer_e2e                         ✅ 真 app HTTP 全链路（init → import → history → detail → 幂等复用 → 404）
cargo fmt --check（5 crate）                                          ✅ 无差异
cargo clippy -p nomifun-importer --all-targets                        ✅ 本方案文件零警告
bun run typecheck（web/）                                             ✅ 通过
```

## 4. 关键安全边界（已验证）

- **零执行**：导入只读取与复制；`walk.rs` 拒绝一切符号链接；socket/fifo/设备文件视为敌意内容整体阻断；
- **凭据**：userConfig 只生成字段 schema（`sensitive` 标记）；`default`/`value` 永不入库；错误与警告统一 `[REDACTED]`；materialized 目录是来源字节的不变镜像（供来源追溯），标准化定义与目录记录不含凭据值；
- **路径**：清单路径必须 `./` 开头、拒绝 `..` 与绝对路径（`validate_relative_path`）；绝对来源路径只存 `source_uri`（内部追溯），不出现在公共协议（02 §9）。

## 5. 已知边界（如实披露）

- 市场来源仅本地目录；GitHub/Git/HTTP 市场源为 `compatible-with-adapter`，未实现；
- 导入后**运行时激活**（Skill 加载、MCP 配置注入、Preset 绑定、TeamRun）属后续 Gate；`runtime_status` 一律 `not-verified`，UI 与协议如实显示；
- 同插件不同版本导入会产生同 `component_id` 的目录条目（`agent/list` 按最新快照优先去重展示；`plugin_snapshot_components.component_id` 未注册为全局 UUIDv7 业务列，v3 契约按本地唯一处理）；
- 插件级 Agent 的 `mcpServers`/`permissionMode` 记录 `ignored-by-source-runtime` 原因码，不映射为授权（02 §5.1）。