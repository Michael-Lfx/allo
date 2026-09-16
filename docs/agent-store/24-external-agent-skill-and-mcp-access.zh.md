# 外部 Agent 使用已安装 Skill / MCP · 实现方案

> 状态：**两个阶段均已落地（阶段 1 2026-09-20 / 阶段 2 2026-09-21）**，阶段 2 的 stdio 会话
> 复用已于同日补上（纯实现层，指纹不变，见 §9.1）。本页是动工前的范围
> 与验收口径登记，实施读数与偏差回写在 §9.1。
> **2026-09-21 做过一次「现状对照」修订**：§5–§7 是**动工前口径**，凡已被实现取代之处
> 均就地标注为「规划期口径 → 见 §9.1」，未改写当时的判断（便于对照），另订正了 §1 表格、
> §5.4/§6 的方法计数、§6 的指纹快照，并补记新门禁 `bun run check:fingerprint`。
> 前置：`05-flowy-agent-store-app-server-protocol.md`、`10-public-contracts.md`、`16-sdk-webui-site-priority-plan.zh.md` §7 决策 4（指纹）、`17-plugin-spec.zh.md` §5/§6、`20-tool-injection-policy.zh.md`、`07-typescript-sdk.md`
> 用途：回答「外部开发者自己实现了 agent，想通过 SDK 使用 Flowy Agent Store 已安装的 skill / mcp」——界定现在能不能、缺什么、怎么补，以及每步的验收标准。

---

## 1. 结论先行

| 目标 | 现状（动工前） | 本方案 | 结果 |
|---|---|---|---|
| 用**专家 / 专家团** | ✅ 已可用（`agent/run` + `mentions`） | 不动 | — |
| 用**已装 Skill 的正文** | ⚠️ 半可用：`skill/get` 只回 ≤1200 字摘要 | **阶段 1**：新增文件读面 | ✅ 已落地（2026-09-20） |
| 用**已装 Skill 的附属文件** | ❌ 完全没有 | **阶段 1** | ✅ 已落地（2026-09-20） |
| 用**已装 MCP / 连接器** | ❌ 拿不到连接参数，且无调用面 | **阶段 2**：调用代理 | ✅ 已落地（2026-09-21） |
| 复用**用户已装**产物 | ❌ `launchClient` 起的是空白实例 | 见 §8 假设 6（不改，只用 `client`） | ❌ 仍未做（有意，非欠账） |

**专家的定位确实是「提示词 + 一个 Nomi runtime 绑定」**（`app_server_installer.rs` 把 agent/team 装成 Preset，绑死 `NOMI_RUNTIME_AGENT_ID`），**而 skill / MCP 是通用产物**——这个区分成立，也是本方案只动后两者的理由。

---

## 2. 边界（非目标）

- **不改 `skill/list` 的 `id` 为 source-qualified id**。它是技能名，`agent/list` 是组件 id，这个不对称真实存在；但改它同时冲击 mention 挂载、写面按名 join、`writable` 判定，属破坏性变更。本次只在正文写明不对称。
- **不复用公开 assets 路由**（`/api/app-server/imports/{snapshot}/assets/{path}`）。它无鉴权 + 图片白名单（为 `<img>` 设计），skill 文件面另开**认证**路由。
- **不开放连接器 transport / env / headers / token 的导出**。「`connector/export`」方案被否决：等于把宿主凭据交给第三方进程。
- **不执行 `scripts/`**。与 `02` §5 / `17` §6 一致：导入只复制不执行；读面只回字节。
- 不新增远程 / 多租户能力，不引入第二套权限模型。

---

## 3. 设计依据（既有事实，附证据位置）

| 事实 | 位置 |
|---|---|
| Skill 是**目录**，附属文件递归拷贝 | `importer/src/install.rs` `copy_dir_into`（递归）；`02` §5「保留 references/scripts/templates/assets」；`17` §5「含 `SKILL.md` 与其附属文件」 |
| `skill/get` 只回摘要，正文被截断 | `app_server_catalog.rs`：`instructions_summary` 走 `.take(1200)` |
| 公开 assets 路由无鉴权、仅图片 | `nomifun-app-server/src/lib.rs` `snapshot_asset_route`：「no connection header, no owner auth」+ MIME 白名单（png/jpg/webp/gif/svg/ico） |
| 路径校验有现成范式 | 同上：`canonicalize` + `starts_with(canonical_base)`，已有穿越测试 |
| 可从落点反推 snapshot/slug | `app_server_catalog.rs::managed_snapshot_provenance` |
| 树摘要算法已存在 | `nomifun-importer::digest::tree_digest_of_dir` |
| **快照 `content_digest` 覆盖整棵来源树**，不是单个技能目录 | `importer/src/import.rs`：`tree_digest(&files)` 作用于整份 import 的文件清单 |
| 连接器 transport 是有损摘要，不可执行 | `nomifun-api-types/src/app_server.rs` 字段注释：「Display-only… Never a raw shell command the client may execute」 |
| MCP 调用能力已存在，但绑在 Nomi 会话上 | `nomi_mcp::manager::McpManager::call_tool`；`NomiAgentManager.mcp_managers` |
| 现成 MCP 客户端只做到 `tools/list` | `nomifun-mcp/src/connection_test/`（stdio/http/sse 三通路 + headers + OAuth 注入已具备） |
| 权限分级已有成熟模型 | `nomifun-gateway`：`DangerTier` / `Surface` / `default_decision` 矩阵 / loopback lease |
| 能力位必须诚实（不广告必然失败的方法） | `Capabilities` + `CapabilityAvailability::from_state`；`approvals` 的注释是既有先例 |
| 路由表计数被测试钉死 | `web/packages/client/src/http-transport.test.ts`：**46 映射 / 22 无 HTTP 绑定**（合计 68） |

---

## 4. 阶段 1：Skill 文件读面

### 4.1 协议面新增 2 个方法

```text
skill/files  { skill_id }        → AppServerSkillFileList
skill/file   { skill_id, path }  → 文件字节
```

```rust
pub struct AppServerSkillFile {
    pub path: String,     // 技能目录内相对路径，POSIX 分隔符
    pub size: u64,
    pub digest: String,   // 单文件 sha256（小写 hex）
}
pub struct AppServerSkillFileList {
    pub skill_id: String,
    pub files: Vec<AppServerSkillFile>,   // 按 path 排序；不列目录
    pub content_digest: String,           // 该技能目录的树摘要（见下）
    pub truncated: bool,                  // 清单触顶，不静默截断
}
```

**`content_digest` 的准确语义**：它是**该技能目录**的树摘要（同 `tree_digest` 规则：排序后的相对路径 + 逐文件 sha256）。**它不等于、也不可互换于**快照的 `content_digest`——后者覆盖整棵导入来源树。两者仅在「快照里只有这一个技能目录、别无他物」时相等。这个区分必须写进正文，否则调用方会拿它去和 `import/get` 对账。

**绑定**：WS arm + HTTP 路由（字节走 HTTP 是自然的）：
- `GET /api/app-server/skills/{skill_id}/files`
- `GET /api/app-server/skills/{skill_id}/files/{*path}`

**能力位**：新增 `skill_files: bool`，`from_state` 取 `state.skills.is_some()`。

**错误码**（进 `10` §7）：`invalid_request`（路径非法）、`not_found`（技能或文件不存在）、`unsupported_operation`（宿主未接 provider）、`response_too_large`（单文件超限）。

### 4.2 seam 划分

在 `nomifun-app-server/src/catalog.rs` 新增独立 trait（不把读写混进 `SkillCatalogProvider`）：

```rust
#[async_trait]
pub trait SkillFileProvider: Send + Sync {
    async fn files(&self, skill_id: &str) -> Result<AppServerSkillFileList, AppError>;
    async fn read(&self, skill_id: &str, path: &str) -> Result<SkillFileBytes, AppError>;
}
```

`AppServerRouterState` 增 `skill_files: Option<Arc<dyn SkillFileProvider>>`（默认 `None`）。**注意**：该结构是公开的，加字段属编译期破坏，需同步所有构造点（含 `catalog.rs` 的 fake 与 `lib.rs` 的测试）。

adapter 落 `nomifun-app/src/app_server_catalog.rs`（与 `AppServerSkillCatalog` 同址，复用 `SkillPaths`）。

### 4.3 实现要点

1. 由 `skill_id` 定位磁盘目录：`list_available_skills` 解析 → `skill_manifest_path(location)` 区分「目录型」与「清单文件型」（builtin 指向文件）。**只有目录型可列文件**。
2. 递归枚举（`follow_links(false)`，与 `importer/src/walk.rs` 同策略），跳过目录项。
3. 逐文件 sha256；`content_digest` 用 `tree_digest_of_dir(&dir)`。
4. **上限**（常量集中一处）：单文件 ≤ 2 MiB（超出 `response_too_large`）；清单 ≤ 2000 项（超出置 `truncated: true` 并停止枚举——列不全比列不出有用，但必须自曝）。
5. **不得把绝对路径加进 wire DTO**。`skill_files` 内部解析路径，对外只给相对路径。

### 4.4 路径与安全检查

照抄 `snapshot_asset_route` 的既有范式：

1. 先做字符串级拒绝：绝对路径、Windows 盘符 / UNC、任何 `..` 段。
2. 技能目录自身做符号链接检查（`refuse_symlink` 策略）。**注意依赖方向**：`nomifun-app-server` 不引入 `nomifun-importer`；检查放 adapter 层，或下沉到双方已共用的 `nomifun-extension::skill_service`。
3. `canonicalize(base)` 与 `canonicalize(base.join(path))`，断言 `target.starts_with(base)`。
4. 目标是**文件**，非目录、非符号链接。
5. 走 `registry.require_ready(connection_id, &user.id)`，与其它 `/api/app-server/*` 一致。

> **安全定位（必须写进正文）**：同机同用户的进程本就能直接读这些文件，所以这一面是**便利性与稳定性边界，不是保密边界**。要求认证 + 穿越校验的目的，是**不让它退化成一个未认证的任意文件读原语**（对标公开 assets 路由的设计约束）。

### 4.5 TypeScript 包

- `protocol.ts`：两个 DTO + `Capabilities.skill_files`。
- `client/src/skills.ts`：`files(skillId)`、`readFile(skillId, path)`。
- `http-transport.ts`：路由表 +2 → **48**；`http-transport.test.ts` 的 `toHaveLength(46)` 同步改 48，`DOCUMENTED_UNMAPPED` 保持 22。
- `sdk/README.md`：增「读技能附属文件」一段。

### 4.6 测试与验收

| 层 | 用例 |
|---|---|
| `nomifun-app-server` 单测 | 穿越（`..` / 绝对路径 / 盘符 / 编码变体）→ `invalid_request`；不存在 → `not_found`；未接 provider → `unsupported_operation`；清单超 2000 项 → `truncated: true`；单文件超限 → `response_too_large` |
| `nomifun-app` e2e | 装一个**带附属文件**的 fixture 技能 → `skill/files` 列出附属文件 → `skill/file` 取回字节逐字相等 → 返回值与独立 `tree_digest_of_dir` 自洽 |
| **反向验证（本阶段的存在理由）** | 断言「长于 1200 字的 SKILL.md：经 `skill/get` 被截断、经 `skill/file` 完整」 |
| web | `cd web && bun run typecheck && bun run test`；`smoke.ts` mock 加断言；路由表计数更新 |

### 4.7 顺带订正（本阶段发现，与本方案直接相关）

1. **`05` §4.8 的 mention 示例 id 自相矛盾**：示例里 `agent` 用组件 id（对），`skill` 也用组件 id（**错**——`skill/list` 公布的是技能名）。照抄会静默不挂载（`SkillId::parse` 失败 → 降级为 `legacy:<组件id>` → 按名查不到）。改为技能名，并补一句「技能 mention 用 `skill/list` 的 `id`，不是 `install/status` 的组件 id」。
2. **`importer/src/install.rs` 的 `materialize_skills_copies_under_managed_prefix` 断言消息过期**：断言与代码一致（附属文件确实被递归拷贝），但消息写着「only SKILL.md is copied」。改正消息，并在 e2e 里把「附属文件存在」变成真断言。

---

## 5. 阶段 2：MCP 工具调用代理

**方向已定**：连接与凭据由 Agent Store 持有，对外只暴露**调用**。第三方因此不接触 transport / env / token。

### 5.1 权限模型（先于协议面定）

对齐 `nomifun-gateway` 的分级思路，但**工具没有 `DangerTier` 标注，所以不猜**——改用宿主显式 allowlist、默认全关：

```toml
# ~/.agent-store/config.toml（与 [tools] 同址、同「只被 apps/agent-store 采纳」规则）
[connector_proxy]
enabled = false                                     # 缺省关；无此表 = 关
allow = ["github__create_issue", "docs__search"]    # 缺省空 = 无工具可调
```

- env 覆盖 `AGENT_STORE_CONNECTOR_PROXY`（JSON，同表形状，**整份替换**），与 `AGENT_STORE_TOOLS` 完全同构——复用既有机制，不发明第二个。
- 判定顺序：`!enabled` → `policy_denied`；工具不在 `allow` → `policy_denied`（**不是** `not_found`：必须能区分「不存在」与「不允许」）；连接器 `enabled == false` → `connector_unavailable`（与 `agent/run` 前置校验同码同义）。
- 审计：每调用一条 `tracing`（principal / connector / tool / 结果 / 耗时 / 字节数）。**不记 arguments 原文**（可能含用户数据），只记 hash 与长度。

### 5.2 协议面新增 1 个方法

```text
connector/call  { connector_id, tool, arguments } → AppServerConnectorCallResult
```

```rust
pub struct AppServerConnectorCallResult {
    pub is_error: bool,               // 上游 isError 原样透出
    pub content: serde_json::Value,   // 上游 content，仅公开面
}
```

- `arguments` 用 `serde_json::Value`（MCP 参数是任意 JSON Schema，不能强类型化）。
- **工具级失败 ≠ wire 错误**：上游 `isError: true` 走 `is_error` 字段让调用方分支；只有连接 / 协议 / 超时才升级为 wire 错误码。
- 能力位 `connector_calls: bool`。错误码：复用 `policy_denied` / `connector_unavailable`；新增 `connector_call_failed` / `connector_call_timeout` / `response_too_large`（后者与阶段 1 共用）。
- 绑定：WS arm + `POST /api/app-server/connectors/{connector_id}/call`。

### 5.3 后端实现

新增 trait `ConnectorCallProvider`（`catalog.rs`）+ `AppServerRouterState.connector_calls`。adapter 落 `nomifun-app/src/app_server_connector_call.rs`。

**客户端生命周期（关键决策）**：「**不复用** `NomiAgentManager.mcp_managers`」这条**始终成立**——它绑在一次 Nomi 会话上，寿命与语义都不对。

> **规划期口径（已被 §9.1 取代，保留以对照）**：本节当时写的是「v1 采用**每次调用一个短生命周期客户端**，与 `McpConnectionTestService::test_connection` 同形；代价（每次一次握手）写进正文，并按显式延后项登记『后续可加按 connector_id 的空闲池』」。
> **实际**：空闲池**后来做了**且**只做 stdio**（`nomifun_mcp::McpToolCallPool`，见 §9.1「stdio 会话复用」）。「每次调用一个短生命周期客户端」并未消失——它仍是 HTTP / SSE 的现行行为，也是池满且都在忙时的回退路径；宿主被强杀时池中子进程可能成为孤儿（与任何 spawn 同暴露面，受池上限约束）。

**最小扩展**：把 `nomifun-mcp/src/connection_test/protocol.rs` 的 `tools/list` 请求构造泛化为 `rpc(method, params)`，新增 `tools/call`，复用其 stdio / HTTP / SSE 三通路与 headers（含 OAuth 注入）。**不新写第二份 MCP 客户端**。

**凭据**：stdio 的 `secret:<KEY>` 引用按 `17` §6 / `21` D5 既有路径解析注入（缺凭据则省略该变量 = fail-closed）；OAuth 复用 `McpOAuthService` 读 token。**token 绝不出现在响应、日志、审计中**。响应只透传上游 `content`，**不附加**任何 transport / headers / env 字段。

**上限与清理**：单次超时默认 30s；结果序列化后 ≤ 1 MiB（超出 `response_too_large`，**不静默截断**）；成功失败都回收连接，并按**进程树**收（不回收的话 stdio 子进程会因管道填满卡死）。

> **规划期口径（已被 §9.1 取代）**：本节当时写的是「**无论成败都关闭客户端**」+「持续抽取 stdout/stderr」。池化后 stdio 的会话在调用之间**保留**，改由**空转 5 分钟 / 上限 8 / 超时或管道断裂即丢弃**来回收；HTTP / SSE 仍是每次调用一个客户端。

### 5.4 TypeScript 包

`protocol.ts` 加 DTO + 能力位；`client/src/connectors.ts` 加 `call(...)`；路由表 +1 → 映射方法数 **48**（总数 **71**），测试计数同步。**`store.ts` 不改**：代理是调用面而非安装面，保持 `StoreClient` 只做生命周期状态机。

### 5.5 测试与验收

| 层 | 用例 |
|---|---|
| allowlist | 未开 `enabled` / 工具不在 allow → `policy_denied`；连接器 disabled → `connector_unavailable` |
| **凭据（最重要）** | **断言式反例**：响应与审计日志中不含 token / headers / env 值 |
| 上限与超时 | 慢响应 → `connector_call_timeout`；超大结果 → `response_too_large`；两者均断言无残留子进程 |
| 集成 | 用 `nomifun-mcp` 既有 e2e 的假 MCP server（已含 stdio/HTTP + `tools/call` 夹具）跑通一次真实代理调用 |

---

## 6. 指纹与跨仓同步

**两阶段各执行一次**（不合并，可分别发布），按 `web/AGENTS.md` §5 四步：

1. 改两个常量 → **旧值全仓 grep** 收尾。重点夹具：`web/scripts/mock-server.ts`、`web/scripts/smoke.ts`、`web/packages/sdk/src/readiness.test.ts`。
   **这步现在有机械门禁（2026-09-21 补）**：`bun run check:fingerprint`（`scripts/check-protocol-fingerprint.mjs`，已进 `bun run check`）按**标识符**比对全部落点——本仓 6 文件 9 处 + 站点 2 处。**任一落点不一致会失败；某个抽取模式一处都匹配不到也会失败**（后者刻意：模式失配意味着门禁其实什么都没查）。新增落点 = 往脚本的 `MIRRORS`（站点 `SITE_MIRRORS`）加一行。详见 `web/AGENTS.md` §5。
2. 正文：`05` 头部指纹 + 对应章节；本文档 §9 进度；`README.md` 本轮记录。
3. **跨仓 `C:\workspace\agent-store-site`**：`content/docs/{zh-CN,en-US}/typescript-sdk.md` §2 常量示例（中英各一处）、**方法计数**（`46 / 68` → 阶段 1 **`47 / 70`** → 阶段 2 **`48 / 71`**——阶段 1 只让**映射**数 +1：`skill/files` 映射，而 `skill/file` 的 HTTP 绑定**非 JSON**，按守卫口径计入**未映射**）、新错误码、`changelog` §4 未发布台账；两语言结构一致。并在 `examples-sdk.md` 补对外用法（现 §9 只有 `skills.list()` 一行）。
4. 完成标准：旧值在代码里归零；`cargo test -p nomifun-app-server` 与 `cd web && bun run typecheck && bun run test` 绿；站点 `check:docs-sync` 报 `0 drift`、`test:docs-sync` 通过。

> **规划期快照（已过时，保留以对照）**：写本文时指纹是 `2026-09-19`（当日 `2026-09-16`），按「只需与上一次不同」预计下一次取 **`2026-09-20`**；落地前核对 `16` §7 决策 4 台账末尾，**以台账为准**。
> **实际取值依次为**：`2026-09-19`（通知 `conversation/list-changed`）→ `2026-09-20`（阶段 1）→ `2026-09-21`（阶段 2）。上面那句「下一次取 09-20」是对的，但没预见到阶段 2 会紧接在次日落地。订正与逐次落点见 §9.1。
> **再之后（2026-09-21 同日）**：形状从日期戳换成 **`fp-<n>` 计数器**，当前值 **`fp-1`**（`2026-09-21` 成了历史值）。理由是日期戳会被误读成发布日期——`2026-…` 既不是变更日也不是发布日。**不改任何 wire 行为**，但严格相等意味着每个客户端都要更新；且这一改**必须在 `beta.4` 之前**做，`2026-09-21` 还没随任何版本发出去。门禁的形状常量是 `scripts/check-protocol-fingerprint.mjs` 的 `FP_SHAPE`。

---

## 7. 风险与失败模式

| 风险 | 处置 |
|---|---|
| 调用代理成为 SSRF / 内网探测跳板 | v1 只允许**已注册**的 MCP server（transport 由宿主配置），调用方**不能**指定 URL；allowlist 默认空 |
| 上游工具结果回吐敏感数据 | 只做体积上限 + 审计，**不承诺**内容脱敏——正文写明，避免误期 |
| stdio 子进程泄漏 / 管道卡死 | 池化后按**空转 5 分钟 / 上限 8 / 超时或管道断裂即回收**，回收走**进程树**；测试断言无残留进程，并用**心跳文件**把「真的被杀」与「因 EOF 自己退出」区分开（§9.1） |
| 每次调用重新握手导致慢 | **已解决（stdio）**：会话复用，实测 **`586 ms → 3 ms`**；HTTP / SSE 刻意不复用（对端可随时作废会话 id），见 §9.1 |
| 技能文件面被误当保密边界 | 正文写「便利性 / 稳定性边界，非保密边界」；认证 + 穿越校验定位为「防止退化成任意文件读原语」 |
| 新增能力位破坏旧客户端 | 纯增量字段；旧客户端忽略未知 capability 与未知方法 |

---

## 8. 假设与待定

1. 两阶段**分别**发布与 bump 指纹，不捆绑。
2. 技能文件面**不区分 origin**（builtin / user / marketplace 均可读）：同机同用户本就能直读磁盘，区分 origin 只增规则不增安全。
3. `[connector_proxy]` 只被 `apps/agent-store` 采纳，与 `[tools]` / `mcp.json` 同规则；桌面 / Web 宿主不采纳。
4. v1 不支持流式 / 长时任务型工具调用（只做一次性请求-响应）。
5. **命名待定稿**：`skill/files`、`skill/file`、`connector/call`、`[connector_proxy]` 均为**建议命名**。按 `10-public-contracts.md` §8 的顺序（先改该文，再同步 schema / SDK / 测试）定稿。
6. **「复用用户已安装产物」不在本方案内**：`launchClient` 起的是自带临时 data-dir 的空白实例（且单实例锁会拒绝已被占用的目录）。要复用用户库，正确姿势是**连已在运行的宿主**——用 `@flowy-agent-store/client` 直接建 `WebSocketTransport`，而非 spawn。此用法应补进站点 `examples-sdk.md`，但**不改 SDK 语义**。

---

## 9. 实施顺序与进度

```text
阶段 1（Skill 文件读面）
 1. DTO + trait + 能力位（nomifun-api-types / catalog.rs）    → cargo check -p nomifun-app-server
 2. adapter（路径安全 → 枚举 → digest → read）                → 单测：穿越 / 上限 / truncated
 3. WS arm + HTTP 路由 + 鉴权                                 → e2e：带附属文件的 fixture 技能
 4. TS 类型 + client 方法 + 路由表计数 + mock/smoke           → cd web && bun run typecheck && bun run test
 5. 正文（05 / README / 本文）+ 指纹 + 跨仓站点                → 旧值归零；站点 0 drift
 6. 订正 05 §4.8 示例 id；订正 install.rs 断言消息             → 重读核验

阶段 2（MCP 调用代理，阶段 1 验收后）
 7. 权限模型（config 表 + env 覆盖 + 审计）                    → 单测：fail-closed 默认全关
 8. nomifun-mcp 的 rpc(method, params) 泛化 + tools/call       → cargo test -p nomifun-mcp
 9. provider + adapter + 超时 / 上限 / 清理                    → 单测：超时 / 超大 / 无泄漏
10. WS arm + HTTP 路由 + 能力位                                → 集成：对既有假 MCP server 真调一次
11. TS + 正文（05 / 06 / 10 / 17）+ 指纹 + 跨仓                → 全链路绿
```

> **第一道真实性闸门**是阶段 1 第 3 步（e2e 证明附属文件真的可读）：它把本方案赖以成立的
> 「Skill 是目录，不只是 SKILL.md」从**读代码结论**变成**实测证据**。

### 9.1 落地记录

#### 阶段 1（Skill 文件读面）—— 2026-09-20 落地

| 层 | 实际改动 | 验证读数 |
|---|---|---|
| DTO | `nomifun-api-types/src/app_server.rs`：`AppServerSkillFile` / `AppServerSkillFileList`（+ `lib.rs` re-export） | `cargo check -p nomifun-api-types` 通过 |
| seam | `nomifun-app-server/src/catalog.rs`：`SkillFileProvider` trait、`SkillFileBytes`、`SkillFileError`、`MAX_SKILL_FILE_BYTES`；`AppServerRouterState.skill_files` | `cargo check -p nomifun-app-server` 通过 |
| 能力位 | `Capabilities.skill_files` + `CapabilityAvailability.skill_files`（独立于 `skills`） | 单测断言「只接目录不接文件面时 `skills=true` 而 `skill_files=false`」 |
| 协议面 | WS arm `skill/files` / `skill/file`（后者 base64）；HTTP `GET /skills/{id}/files` 与 `/skills/{id}/files/{*path}`（后者回原始字节） | 131/131 lib 测试通过（含 4 个新增） |
| adapter | 新增 `nomifun-app/src/app_server_skill_files.rs`（列清单 / 读单文件 / 路径安全 / 树摘要）；组合根接线 | 10/10 adapter 单测通过 |
| e2e | 新增 `crates/backend/nomifun-app/tests/skill_files_e2e.rs`（3 例，含清单、字节、截断对照、穿越拒绝、未握手拒绝） | 3/3 通过 |
| TS | `protocol.ts` 三类型 + `Capabilities.skill_files`；`client/src/skills.ts` 增 `files` / `readFile` / `readFileWithType`；`http-transport.ts` 加 `skill/files` 路由 | `bun run typecheck` 0 错误；495/495 测试通过 |
| 指纹 | `2026-09-19` → `2026-09-20`（8 处本仓落点） | 旧值在代码中归零（仅剩历史散文 4 处） |
| 计数守卫 | `http-transport.test.ts`：`46 / 22` → **`47 / 23`** | 通过 |

**与方案的偏差（2 处，均为实现中发现的必要调整）**：

1. **`skill/file` 不进 JSON 传输的路由表**。方案原写「HTTP 路由 +2」。实现时确认
   `HttpTransport` 是 **JSON 绑定**（`accept: application/json`、`response.text()`），
   而该路由的产品语义是**回原始字节**，二者不可调和。因此：HTTP 路由**照样存在**（给
   `fetch` 直连用），但 typed 客户端不为其建路由，`skill/file` 计入
   `DOCUMENTED_UNMAPPED`（22 → 23）。WS 侧同一方法回 base64 JSON，`skills.readFile()`
   走的就是它。
2. **`SkillFileError` 而不是 `AppError`**。方案原写「错误码进 `10` §7」，隐含经
   `AppError` 映射。实现时确认 `AppError` 无自定义 code 载体，且它有 **13 处穷尽 match**，
   加变体会波及全仓；而 `response_too_large` 必须与 `invalid_request` / `not_found`
   区分开。故 seam 自带一个小错误类型，协议层映射（`skill_file_error`）。
   **`10` §7 因此不需要改**——wire code 由协议层产生，不新增 `AppError` 变体。

**实现中的两个发现**：

- **`Path::components` 会归一化内部 `.`**，故 `a/./b` 到达守卫时已是 `a/b`。这不是漏洞
  （它解析在技能目录内，且 canonicalize + `starts_with` 是真正的边界），但方案 §4.4 的
  措辞「任何含 `..` 的段」需要连带理解为「`.` 只对**前导**位置构成拒绝理由」。已改代码
  注释、测试与 `05` §4.3.1 三处口径一致。
- **方案 §4.6 计划断言「`content_digest` 等于该快照的 `content_digest`」是错的**：快照摘要
  覆盖整棵导入来源树（`import.rs:222` 的 `tree_digest(&files)`），而本面是**单个技能目录**
  的摘要，两者只在「快照恰好只含这一个目录」时相等。已在 DTO 文档注释与 `05` §4.3.1
  写明，e2e 改为断言与独立 `tree_digest_of_dir` 自洽，而非与快照相等。

**顺带修复（既有缺陷，因本阶段成为必经路径）**：`tests/common/mod.rs` 的
`build_app_with_skill_paths` 用 `AppConfig::default()`（相对 `work_dir`），
`create_router_with_states` 拒绝非绝对 workspace 根，故该 helper **每次调用都 panic**。
改为绝对 `data_dir` / `work_dir`。

**订正（2026-09-21）**：上面这段的原始理由是「唯一使用它的 `extension_e2e.rs` 未登记进
`Cargo.toml`，从不参与编译」——**这条是错的**，我当时只 grep 了 `[[test]]` 的 `name`，漏了
`tests/suites/content.rs` 的 `grouped_tests!`：`extension_e2e.rs` 是以**模块**形式编进
`content_e2e_suite` 目标的，**一直在编译、一直在跑**，并在 **13 处**调用这个 helper。所以那处
panic 一直在让这 13 条测试失败，helper 修好后 `extension_e2e` 实测 **49 passed / 0 failed**。
`Cargo.toml` 里那句「a new top-level test file cannot be silently omitted」的保证是真的：
`tests/suites/content.rs` 有一条 `every_top_level_integration_test_is_registered` 门禁，
把「已登记 + 分组」与磁盘上的文件做**集合相等**断言。

同族的 `build_app_with_noop_opener`（`shell_e2e` 用）本来带**同一个缺陷**，那批测试每次都在
`routes.rs:1033` 以 `workspace policy denied: configured workspace root must be absolute`
panic。**已修（2026-09-21）**：同一处理（`build_app_with_mock_version`、
`build_app_with_mock_agents` 也在内），`shell_e2e` **27/27**、`system_version_e2e` **5/5**、
`message_e2e` **38/38**（后两者的失败也来自同一批 helper）。`shell_e2e` 里另两条 STT 失败是
**另一个**原因：它们给平台 `openai` 的 provider 传了裸 origin，而约定是 `base_url` **自带
`/v1`**（适配器自己的单测就这么写），于是请求打到 `/audio/transcriptions`、被 mock 回 404、
再被 app 映射成 502；`st7` 更是一直**空过**（它只断言 502 + `STT_REQUEST_FAILED`，404 也满足）。

#### 阶段 1 真实链路验收（2026-09-20，实测）

单测与 e2e 用的都是**手工铺的技能目录**；为排除「只在我造的夹具上成立」，另跑了一次
**经公开 SDK 面 + 真实市场条目** 的验收（临时宿主，独立 data-dir，端口 8799）：

| 观测 | 读数 |
|---|---|
| 握手能力位 | `protocol_version = 2026-09-20`、`capabilities.skill_files = true` |
| 市场镜像 | 冷启动异步注册；`markets_pending=true` 直到约第 4 次轮询（~45s）才出现 **262** 个 skill 条目 |
| 目标条目 | `workbuddy-skills/腾讯文档`（真实市场第 1 条） |
| 安装 | `ok=true`、`installed_count=1`；`install/status` → `skill tencent-docs state=installed` |
| `skill/list` | `id=tencent-docs origin=marketplace writable=false` |
| `skill/files` | **11 个文件**：`SKILL.md` + `_skillhub_meta.json` + 9 个 `references/*.md`；`truncated=false` |
| `skill/file` | 取回 `_skillhub_meta.json`，字节数与清单声明一致，**且 sha256 与清单 digest 相等** |
| **不对称验证** | `skill/get` 摘要 **1201** 字符 vs 经 `skill/file` 的 `SKILL.md` **9257** 字符 |
| 拒绝面 | `../outside.txt` / `/etc/passwd` / `..\outside.txt` / `a/../../b` 全部 `invalid_request`；未知技能 `not_found` |

**结论**：真实市场技能（含 9 个附属 reference 文件）确实随安装落地、可列、可逐字读、
digest 自洽，且 `skill/get` 的截断与 `skill/file` 的完整在真实数据上可观测。这条链路
此前只在读代码层面成立。

**一个非本方案的观察——已于 2026-09-21 修复**：纯技能条目的 `store/install-entry` /
`install/run` 响应**不带任何 outcome**（`app_server_installer.rs` 只在 `skipped` / `failed`
分支 push outcome，技能成功路径只 push `pending`），于是 `installed_count: 1` 与
`outcomes: []` **互相矛盾**——按协议读 `outcomes` 的调用方会把一次成功安装读成「什么都没装」。
客户端 `store.install` 的 `outcome.components` 对纯技能条目因此是空数组。

**修法**：技能分支补齐与其它 kind 同形的 `ok_outcome`，`action` 按
`installed_state` 的 `installed` 位取 `created` / `reused`（与 agent/team 的 `reused`
同源信号：已登记的组件是「原地重新物化」，不是新造）。**测试**：新增
`importer_e2e` 的不变量断言——**每个已登记的 skill/agent/team/connector 组件都必须在
`outcomes` 里出现**。这条断言比原来的「非空」强：原测试用的是含 agent 的
`team-tools` 夹具，agent 的 outcome 把技能的空缺**盖住了**，所以它一直是绿的。
已实测该断言在缺 `push` 时会失败（临时去掉一行复现），并指名到具体组件。

**指纹不变**：没有新增方法、也没有给 DTO 加字段——`outcomes` 一直在协议里（`05` §4.5.2），
只是技能分支没往里写。这是让实现回到 `02` §4.7 与 `05` §4.5.2 **已经写明**的
「逐组件回报」契约，所以不 bump 指纹。

#### 阶段 2（MCP 调用代理）—— 2026-09-20 当时状态（次日已落地，见下节）

**已完成：MCP 工具调用能力（方案 §5.3 的「最小扩展」，计划步骤 8）。**

`nomifun-mcp` 的 `McpConnectionTestService` 新增 `call_tool(transport, tool, arguments)`，
覆盖 **stdio + Streamable HTTP**，返回 `McpToolCallOutcome { is_error, result }`
（`result` 是上游结果对象**逐字透传**；只把 `isError` 抬成一个字段）。

三个决策及其理由：

1. **扩这个客户端，而不是新写一份**。它已经拥有代理需要的全部东西——`secret:NAME` 的
   env 解析、进程树回收、HTTP session id、OAuth bearer 注入**与 401 刷新重试**。
   `nomi_mcp::McpManager` 虽然也有 `call_tool`，但它是**agent 侧**crate（后端直接依赖
   `nomi-*` 需要 feature gate，`AGENTS.md` 有明文约束），且它的 server 寿命绑在一次 Nomi
   会话上——复用它会把那份生命周期（以及随之而来的池失效问题）引进一个**没有会话**的面。
   这也是方案里那条「不复用 `NomiAgentManager.mcp_managers`」的落地。
2. **每次调用一个短生命周期会话**。代价是每次一次握手；按方案登记为**已知代价**，
   空闲池是显式延后项，不进 v1。**（→ 次日实现，只做 stdio，见下两节）**
3. **`run_stdio_tool_call` 与 `run_stdio_protocol` 并列，而不是把后者参数化**：探针的
   分阶段 error `details` 是它既有行为的一部分，合成一个形状会改掉它。framing / 消息构造 /
   握手三者共享——**一个 MCP 客户端两个入口，不是第二份客户端**。

**一处已订正的偏差（值得记住的教训）**：起初 **SSE 未实现、按名拒绝**，我写的理由是
「仓库里**没有 SSE 夹具**，实现一条无法验证的通路比显式拒绝更糟」。**这条理由是错的**——
`tests/connection_test_integration.rs` 里就有一个进程内 SSE 夹具（TCP 监听 +
`read_http_request`，不需要 shell），而我为了写调用面**已经读过那个文件**，却没意识到它同样
可用于调用面。既然可验证，就没有理由不做：**SSE 随即实现并验证**（3 条集成测试：成功调用、
工具级失败、服务端 JSON-RPC 拒绝），`call_tool_inner` 的拒绝分支与对应单测一并删除，改为一条
「SSE 走到传输层而不是被按名拒绝」的回归守卫。

教训：**「无法验证」这个判断必须先把夹具搜过再下**——否则它很容易变成省事的借口，而且会写进
规格文档当成设计理由。

**验证读数**：`cargo test -p nomifun-mcp` → lib **258 passed / 0 failed**（较改动前 251
增 7 条），全部集成套件同样通过。7 条新单测里有 **1 条是真实 HTTP 往返**——用 in-crate 的
假 MCP server（axum，`dev-dependencies` 已有）跑通 `initialize → initialized → tools/call`，
因此这条通路在 **Windows 本机**就验过了，不只靠 unix CI。其余覆盖：请求构造、`isError`
抬取与 `result` 逐字不改、工具级失败**不**升级为传输错误、JSON-RPC 错误升级为 `Failed`、
超时命中预算、SSE 按名拒绝。

**仍未做（计划步骤 7 / 9 / 10 / 11）**：权限模型（`[connector_proxy]` 表 + env 覆盖 +
审计）、provider + adapter、WS arm + HTTP 路由 + `connector_calls` 能力位、TS 客户端与
正文/指纹/跨仓。这些是**一次耦合的 wire 变更**——按 `web/AGENTS.md` §5，路由与方法落地
必须与指纹 bump 同步，**所以不在能力尚未接通时先改一半**。

#### 阶段 2（MCP 调用代理）—— 已落地（2026-09-21）

上面那条「仍未做」在 2026-09-21 一次做完（步骤 7 / 9 / 10 / 11 同批，因为它们是**一次耦合的
wire 变更**）。要点：

| 层 | 实际改动 |
|---|---|
| 权限模型 | `[connector_proxy]` 表（`agent_store.rs`）：`enabled` + `allow`；`ConnectorProxyPolicy` 的 `decide(id, name, tool)`；`AGENT_STORE_CONNECTOR_PROXY` env 覆盖；`AppServices.connector_proxy_policy` |
| 协议面 | `connector/call`（WS arm + `POST /connectors/{id}/call`）；`connector_calls` 能力位；`ConnectorCallError` → 6 个稳定码 |
| adapter | `nomifun-app/src/app_server_connector_call.rs`：三道门 → 调用 → 体积上限 → 审计 |
| TS | `ConnectorCallResult` + `Capabilities.connector_calls`；`connectors.call()`；路由表 +1 |
| 指纹 | `2026-09-20` → `2026-09-21`；计数守卫 `47 / 23` → **`48 / 23`** |

**几个刻意的设计选择（都有理由，不只是实现细节）**：

1. **默认全关，且 fail-closed**（与 `[tools]` 的 fail-open 相反）。`[tools]` 只做**减项**，
   坏文件最坏是「没减成」；这张表做**授权**——让第三方能在宿主的连接上执行工具。所以缺表、
   缺 `enabled`、`allow` 为空、env 覆盖解析失败**一律拒绝**。一个手滑不能是「什么都不能调」
   与「什么都能调」的差别。
2. **allowlist 同时接受连接器 id 与注册名**。计划里只写了名字；实现时补了 id，因为 MCP server
   按**名字** upsert（`connector_name_collision` 是既有可观测 outcome）——后来安装的同名连接器
   会接管名字并因此**继承授权**。id 是那条会存活下来的 pin。
3. **请求面 `deny_unknown_fields`**：给 `url` / `command` / `headers` / `env` 一律
   `invalid_request`。所以这条通路**不构成 SSRF**——地址永远来自宿主自己的配置，调用方只能
   点名一个已注册的 `connector_id`。这是计划里「调用方不能指定 URL」那条的落地形式。
4. **`result` 而不是计划里的 `content`**：实现发现透传**整个结果对象**才是不丢字段的做法
   （`structuredContent` 及将来的字段都该活下来），叫 `content` 会误导。**命名偏差，已记。**
5. **不给结果脱敏**：MCP 结果是任意 schema，半脱敏的载荷比经审计的原样载荷更危险。只做体积
   上限 + 审计（不记 arguments）。
6. **三种传输全部支持**（stdio / Streamable HTTP / SSE）。SSE 起初被我以一条**错误**理由
   跳过，见下。

**补上一处我自己的遗漏**：阶段 1 引入 `response_too_large` 时**没有**登记进 `10` §7，理由是
「没新增 `AppError` 变体所以不必改」——那把**实现层枚举**当成了 **wire 公共码**，`10` §7 管的
是后者。本轮连同 `connector_call_timeout` / `connector_call_failed` 一起补齐。

#### stdio 会话复用（2026-09-21 追加，wire 面不变）

阶段 2 把「按连接器 id 的空转会话池」明确列为**延后**项。本轮把它做掉了——**只做 stdio**：

| 层 | 改动 |
|---|---|
| 会话 | 新 `connection_test/session.rs`：`StdioToolSession` 持有子进程与两条管道，握手一次、可反复调用；`StdioCallError` 区分「服务端答复了（`Rejected`）」与「管道状态未知（`Broken`）」 |
| 池 | 新 `connection_test/pool.rs`：`McpToolCallPool`（空转 5 分钟、上限 8、`close_all`） |
| 接线 | `McpConnectionTestService` 保持**无状态传输层**（`call_tool` 仍是一次调用一个会话），池是它上面的**缓存装饰器**；adapter 改为持有池并传入连接器 id |
| 旧路径 | `run_stdio_tool_call` 退役：一次性路径改为「建会话 → 调一次 → 关」，**与池共用同一份握手与回收**，两条路不会各自漂移 |

**为什么只池化 stdio**：因为**只有它是我们拥有的**。stdio 子进程是我们 spawn 的，生命周期
完全可控；HTTP/SSE 的会话 id 由**对端**决定何时过期（规范里对陈旧 id 的答复是 404），缓存它
等于用「稳定成功的调用」换「省一次往返」，而 `reqwest` 本就在底下复用 TCP/TLS。所以规则是
**池化我们拥有的，不缓存对端能背着我们作废的**。这不是范围妥协，是可靠性取舍：远端复用会
让本来必然成功的调用变成偶发神秘失败。

三条不显然但必须有的规则（都有对应测试）：

1. **身份 = 连接器 id + 配置与凭据**。用 id 而非注册名，理由与 allowlist 收 id 相同（MCP
   server 按名字 upsert，后来者会继承前一个的会话）；env 按 `secret:` **解析后的值**比较，
   所以轮换凭据会换新会话，而不是拿旧凭据继续跑。
2. **超时或断裂必须丢弃会话**。这是池化**引入**的新风险：调用超时后可能还有一个答复在管道里，
   复用会让**下一次**调用读到上一次的答复并归错因。一次性调用没有这个问题（会话随调用消亡）。
   反过来，工具级失败（`isError`）与 JSON-RPC 错误都是**成帧的答复**，会话仍在同步状态，保留。
3. **池满且都在忙时退回一次性调用**。复用是优化，从来不是正确性前提。

**验证**：新增 `tests/fixtures/fake_stdio_mcp.mjs`——一个跨平台的**真** MCP stdio server
（既有 stdio 夹具是 `#[cfg(unix)]` 的 `sh -c`，在 Windows 上跑不了，而池化的收益全在 stdio）。
它把事件追加到 `$FAKE_MCP_LOG`，所以「会话被复用」是**问服务端**问出来的，不是信实现：
`initialize` 每次进程启动只出现一次。新增 **12 条**集成测试（该文件 13 → 25 条）：复用 /
答复归属 / 空转过期 / 改配置换会话 / 轮换凭据换会话 / 工具级失败保留 / JSON-RPC 拒绝保留 /
超时丢弃 / 管道断裂丢弃 / 池满回退 / `close_all` 清空 / `close_all` **杀掉进程**。

**这一条验证值得单说**，因为「没有残留进程」并不等于「杀掉了」：子进程也可能因为管道 EOF
自己退出。夹具的 `stubborn` 工具在答复后**停止读 stdin 并持续写心跳文件**，所以关闭管道杀不死它
——心跳停了只可能是真的被杀。我先**单独验证了这把尺子**（直接跑夹具、关 stdin、确认心跳仍在
增长），再让它进测试；否则「心跳停了」可能只是因为 EOF。

**一处我自己造出来的假失败**：超时测试最初给 1500ms 预算，冷启动（22 条测试并行 spawn bun）
时会不够，于是偶发失败。首次失败时我差点当成实现 bug——日志显示只有**一个**进程、`hang` 根本没
送达，说明是**测试设计**问题：预算既要覆盖冷启动又要覆盖故意挂起。已改为 4s（远高于实测的
几十毫秒启动）并补了一条**不依赖时序**的断裂测试。

**指纹不变**：没有新增方法、字段或错误码，所以不 bump 指纹、不动站点仓——这是纯实现层改动。

**真实链路读数**（`agent-store.exe` + 本地 SDK，2026-09-21 实测）：用
`POST /api/mcp/servers` 注册那个 stdio 夹具连接器（新建行默认 `enabled=false`，需再 toggle
一次——这是既有契约，不是本次改动），`AGENT_STORE_CONNECTOR_PROXY` 开代理并只放行
`live-connector__echo`，然后走两次真实 `connector/call`：

| 读数 | 值 |
|---|---|
| call#1 耗时 | **586 ms**（冷启动：spawn + 握手） |
| call#2 耗时 | **3 ms**（复用） |
| 夹具日志 | `start` ×1、`initialize` ×1、`call` ×2，两条 `call` **同一个 pid** |
| 两次结果 | 分别 `{"n":1}` / `{"n":2}`——**没有归错因** |
| `server.close()` 之后 | 无残留 `bun` / `agent-store` 进程 |

这一条是**接线**验证，单测覆盖不到：adapter 若每请求新建一个池，读数会是 2 次 `initialize`；
共享的池在 router 构造期只建一次（`routes.rs` 的 `AppServerConnectorCall::new`），实测坐实了
这一点。

**仍未做（登记）**：HTTP/SSE 会话复用（按上面的理由**刻意不做**，不是欠账）；池是每个 adapter
一份（每个宿主进程一份），不是跨进程共享；宿主被**强杀**时池中的子进程可能成为孤儿——与任何
spawn 的暴露面相同，且被池上限约束。

### 9.2 本方案提出时已实测 / 未实测

- **已实测（读代码确认）**：目录递归拷贝、`take(1200)`、assets 路由无鉴权且图片白名单、`McpManager::call_tool` 存在、MCP 会话 per-Nomi-agent、`managed_snapshot_provenance` 可反推 snapshot、快照 `content_digest` 覆盖整棵来源树。
- **已由阶段 1 e2e 转为实测**：技能附属文件真的随安装落在受管目录、且经协议可读；
  `skill/get` 的摘要确实截断而 `skill/file` 完整；穿越与未握手访问确实被拒。
- **提出时仍未实测**：任何 MCP 调用（阶段 2）——阶段 2 落地后**已实测**（见 §9.1 的 live 9/9
  与上文的 stdio 池化测试）。这一行保留为**提出时**的快照，不代表当前状态。
