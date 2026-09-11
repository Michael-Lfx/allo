# 插件规范（兼容层）

> 状态：**现行正文（未正式发版，可改；改动同步更新）**——本规范在发版前只有一个版本（统一称 v1），不设 v1/v1.1/v2 之分（`16-sdk-webui-site-priority-plan.zh.md` §7 决策 4）。覆盖范围：字段表（§3）、组件映射（§4）、版本/溯源/幂等（§5）、权限与安全边界（§6）、依赖与冲突（§7），以及机器可校验形态（`docs/agent-store/schemas/plugin.schema.json` + `scripts/check-agent-store-market.mjs`）。
> 已知偏差见 §10：**P2 已修**；**P1 / P3 / P4 为未实现项**（在唯一现行版本内补齐，无版本路径）。**保留的约束**：任何变更走**显式修订 + 偏差登记**，不静默修改本规范。
> **定位：本规范只定义「兼容层」。** 依 `16` §4 Q5 的决策——**先只做兼容层**——Agent Store **尚未定义原生插件格式**；此处规定的是「Agent Store 当前接受什么、如何归一化、边界在哪」，而非插件作者应当遵循的自有格式。
> 字段级映射详表见 `02-codebuddy-workbuddy-import-spec.md`（本规范不重复，只固定必填/可选/默认与冲突规则）。
> 代码事实来源：`crates/backend/nomifun-importer/`、`crates/backend/nomifun-extension/`、`crates/backend/nomifun-api-types/src/app_server.rs`。

---

## 1. 定位与非目标

**当前接受的格式**是 CodeBuddy / WorkBuddy 生态的既有格式（`.codebuddy-plugin/`、`.codebuddy-skill/`、`.codebuddy-connector/`）。Agent Store 的角色是**归一化 + 快照**，不是定义格式。

因此：

- 字段语义、默认值、保留位**以兼容源为准**；Agent Store 不擅自扩展语义；
- 兼容源新增字段时，Agent Store 的行为是「保真透传展示元数据」或「忽略」，**不猜测**；
- 一旦未来定义原生格式，本规范以**新增章节或独立文档**承载该格式，兼容层降为「导入源」并保留（这是**范围**问题，不是版本升级）。

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

## 3. 清单字段：必填 / 可选 / 默认

三份清单按 §2 的路径发现。**`name` 是唯一必填字段**（空白视为缺失，导入阻断为 `MissingIdentity`）；其余全部可选且有明确默认。

| 字段（wire key） | 必填 | 类型 | 默认 | 处理 |
| --- | --- | --- | --- | --- |
| `name` | ✅ | string | — | 唯一必填；参与 ID 派生（§5）；空白 → 阻断 |
| `version` | — | string | 无 | 记为 `declared_version` |
| `description` | — | string | 无 | 展示 |
| `author` | — | string \| `{name, email}` | 无 | 对象形式归一化为人名（有 email 时附上） |
| `agents` / `skills` / `commands` | — | string \| string[] | `[]` | 单字符串与数组都接受；`agents`/`skills` 参与归一化，`commands` 仅记录 |
| `hooks` | — | object | 无 | 仅记录，**不启用** |
| `mcpServers` | — | object | 无 | 参与归一化 |
| `lspServers` | — | string \| object | 无 | V1 元数据级（仅保留服务器名清单），**不启用** |
| `userConfig` | — | object | 无 | 配置 schema → `CredentialSchema`；敏感值仅引用安全存储 |
| `dependencies` | — | array \| object | `[]` | 对象形式按 kind 展开为条目并附 `group` 标记（§7） |
| `teamInfo` | — | object | 无 | WorkBuddy 扩展 → `AgentTeamDefinition`（`02` §6） |
| `displayName` / `profession` / `displayDescription` / `defaultInitPrompt` | — | localized | 无 | 展示元数据，**保真透传** |
| `quickPrompts` / `tags` | — | localized \| localized[] | `[]` | 同上 |
| `avatar` | — | string（相对路径） | 无 | 资产随快照复制，经受控端点 serve |
| `expertType` / `categoryId` / `agentName` | — | string | 无 | 展示 / 分类 |
| `defaultEnabled` / `channels` | — | — | — | 安装 / 启用策略元数据 |
| `strict` | — | bool | `false` | `true`：要求插件源自带 `plugin.json`；`false`：市场条目可补充或代替清单 |

**宽容解析规则**（兼容源真实数据形态不一，以下差异一律归一化，不报错）：

| 字段 | 接受形态 |
| --- | --- |
| `author` | `"张三"` 或 `{"name": "张三", "email": "…"}` |
| `agents` / `skills` / `commands` | `"./agents/"` 或 `["./agents/a.md"]` |
| `dependencies` | 数组 `[{name}]` 或按 kind 分组的对象 `{connectors: ["x"]}` |
| 展示元数据 | 字符串或按语言的对象 |

> **字段级唯一正文是 `02-codebuddy-workbuddy-import-spec.md` §4 / §5**（含每个字段的来源与保留清单）；本规范只固定**必填性、默认值与宽容规则**。

> ⚠️ **`strict` 当前未实现**：既不解析该字段，也没有「缺 `plugin.json` 即阻断」的路径（见 §10 P4）。

**冲突规则**：市场条目字段与插件清单字段合并时逐项比对，冲突以**插件清单为准**并记录冲突（不静默覆盖）。

**真实市场已出现、本规范未消费的字段（普查 2026-09-10，T20）**——下列字段在当前实现里**透传但不消费**（宽容解析接受，不参与归一化）。登记在此是为了让字段表与真实数据一致；若要真正消费其中某一项，按**显式修订**定义：

| 字段 | 出现处 | 现状 |
| --- | --- | --- |
| `plugin` | experts 市场内 7 个插件清单 | 透传（未消费） |
| `members` | 3 个专家团队的插件清单 | 透传（团队扩展以 `teamInfo` 为准） |
| `license` / `homepage` / `repository` | 少量插件清单 | 透传（未消费） |
| `distribution` | 1 个插件清单（`{channels, primaryChannel}`） | 透传（未消费） |
| `settings` | 1 个插件清单（`{defaultAgent, channelManifest}`） | 透传（未消费） |

> 复现：`node scripts/check-agent-store-market.mjs --census --market <name>=<dir>`（输出里 `?` 前缀即本表来源）。市场清单层面的同类字段（如 `owner`）登记在 `18` §11 D3/D7。

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

> ⚠️ **当前现状（T20 反向验证，2026-09-10）**：`userConfig` 路径符合本节（只建 schema、值写 `[REDACTED]`）；但 **MCP 连接器的 `env` 值会原样写入快照**——§10 登记为 **P1**，是必须与实现同批收口的**前置项**（需要先有「安全存储注入」这条路才能真正占位化）。

---

## 7. 依赖与冲突

- `dependencies` 支持**字符串**或**对象**（`name` + `version` + 可选 `marketplace`）；
- 版本使用 **SemVer 范围**；
- 依赖不可满足时：阻断安装并给出缺失项，不做静默降级；
- `strict=true` 且插件源缺 `plugin.json` → 阻断（见 `02` §11.1 阻断规则）。

> ⚠️ **当前现状（T20 反向验证，2026-09-10；2026-09-11 补层级界定）**：**导入层**（本规范约束的这一层，`nomifun-importer`）当前只**登记**依赖声明（数组与按 kind 分组两种形态均已归一化，见 §10 P2），**不解析 SemVer 范围、也不因依赖不可满足而阻断**（§10 P3）；`strict` 阻断同样未实现（§10 P4）。**层级界定**：本段结论**仅对导入层成立**——`nomifun-extension` 的 extension 层**另有**一条链，已用 `semver::VersionReq` 解析并已接进真实加载路径（详见 §10 P3 的证据列），两层的现状不同，不可互相代入。这三条在**本规范内直接补齐**（无版本路径）。

---

## 8. 演进

- 原生插件格式**待生态起量后**再定义；届时以**新增章节或独立文档**承载，并明确原生格式与兼容层的字段优先级（这是范围扩展，不是版本升级，故不涉及迁移路径与弃用窗口）；
- 在原生格式定义前，**不接受**任何以 Agent Store 名义扩展的私有字段。

---

## 9. 验收口径

按本规范 + `02`，能够**独立复现**一个可被 `market/add` → `market/refresh` → `store/list` → 安装 的插件包，且现有真实市场（experts / skills / connectors）逐条对照无例外。字段级细节以 `02` 为唯一正文。

**机器可校验形式（D2 / T19，2026-09-10）**：`docs/agent-store/schemas/plugin.schema.json` 是本节的机器可读版本（唯一必填 `name`、宽容形态、`avatar` 相对路径约束）。复核方式：

```bash
node scripts/check-agent-store-market.mjs --market experts=<dir> [--market …]   # 三个真实市场应 0 error
node scripts/check-agent-store-market.mjs --self-test                          # 非法样例必须被拒并给出字段级定位
```

---

## 10. 已知偏差

**登记规则（强制）**：规范与实现不一致时，**先在本节登记，再择一修正**——要么改实现，要么改规范，不允许默默不一致。每条偏差须写明：现象、证据位置、影响、待决选项。

| # | 现象 | 证据 | 影响 | 待决 |
| --- | --- | --- | --- | --- |
| P1 | **MCP 连接器的 `env` 原样写入快照**（§6 要求敏感值只留引用） | `import.rs:1100-1112`：`config.env` 直接进 `transport.env` 并写入组件 payload；测试 `importer_tests.rs:435-439` 断言明文 `DEMO_TOKEN="demo-token"` 落库。（同 crate 的 `userConfig` 路径是合规的：`import.rs:627-662` 只告警并写 `[REDACTED]`） | API Key / Token 随快照持久化，违反 §6「导入时只建立 schema 与引用」 | ⏸ **建议 ② 改规范（记当前现状）+ 列前置项**：① 值占位化依赖「用户自己的安全存储注入」这条路径，当前不存在——直接写 `[REDACTED]` 会让 MCP 服务器拿不到凭据而启动失败，是功能回归。**安全优先级最高**，两步须同批 |
| P2 | **数组形式的裸字符串依赖丢名** | 旧 `manifest.rs` 数组分支原样透传 → 消费端 `import.rs:667-670` 取 `name` 失败，回退 `dependency-<index>`；`version` / `marketplace` 也未解析 | 快照里的依赖成了无名条目，用户看不出依赖了什么 | ✅ **已修（2026-09-10，T20）**：新增 `normalize_dependency`（裸字符串 → `{"name": …}`），数组与按 kind 分组两种形态共用；单测 `bare_string_dependencies_keep_their_name`；`cargo test -p nomifun-importer --lib` **25 passed** |
| P3 | **依赖的 SemVer 范围与「不可满足即阻断」未实现——⚠️ 仅限「导入层」，须与 extension 层分开登记** | 导入层：`import.rs:664-681` 只把依赖登记为组件，从不参与安装决策（**此处「全仓无 `semver` 解析」的结论只对本消费面成立**）。extension 层**另有**一条链：`nomifun-extension/src/dependency.rs:112-129` 用 `semver::VersionReq` 解析（裸版本=精确 / `^` / `~`）且**已接进真实加载路径**（`registry_helpers.rs:46-61` 的 `load_and_validate` 在 `:56` 调用、`:57` 用 `load_order` 排序 ← `registry.rs:111` / `:166`），**只缺「阻断」**（`registry.rs:136-138` 只 `warn!`，`valid` 未当门） | §7 的版本范围与阻断语义当前只有「登记」这一半；**两层现状不同**，若混写成一句会互相打脸（`16` R23 行原即如此，已订正） | ⏸ **待补齐（在本规范内，无版本路径）**：§7 的满足性解析与阻断**直接补齐**；真实工作量**不是「实现 SemVer 解析」**——导入层需新写校验，extension 层只需补阻断判定与逃生口（`21` D6=A 已批准；**规范未发版、无既有消费者，故阻断默认开 + 逃生口**，见 `16` R23 行） |
| P4 | **`strict` 完全未实现**（**属导入层**，受理应与 P3 同批） | `PluginManifest`（`manifest.rs:122+`）无 `strict` 字段，全仓无相关解析或阻断路径；`nomifun-extension` 内搜 `strict` 仅命中 template 模板的无关用法（**不是同一物**） | §3 字段表承诺的「`strict=true` 要求插件源自带 `plugin.json`」与 §7 的阻断规则（`02` §11.1）当前无法兑现 | ⏸ **待补齐（在本规范内，无版本路径）**：§3 / §7 与实现同批补齐 `strict` 阻断；实现需打通导入期阻断路径（与 `MissingIdentity` 同级）。**决策已备**：`21` D6=A 已批准（含 `strict`），故不是待拍板项，只等排期（见 `16` 卡点决策表 R23/R24 行） |

> 说明：`02` §8 的「自动更新默认值」偏差登记在 `18-marketplace-spec.zh.md` §11（属市场行为，不属插件格式）。
