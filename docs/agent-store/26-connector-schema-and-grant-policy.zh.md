# Connector 工具签名读面与授权形状 · 技术方案

> 状态：**已落地**（2026-09-22 起草并实施；逐层改动、真实读数与偏差见 §9.1）。
> 前置：`05-flowy-agent-store-app-server-protocol.md`、`10-public-contracts.md`、
> `20-tool-injection-policy.zh.md`、`24-external-agent-skill-and-mcp-access.zh.md`（§5 调用代理）、
> `web/AGENTS.md` §5（指纹与跨仓同步）。
> 用途：回答两件事——**(1)** 取消「逐工具 allowlist」之后，`connector/call` 靠什么授权；
> **(2)** 外部调用方从哪里拿到 MCP 工具的**参数 schema**。并记录一个**决定不改**的命名。

---

## 1. 结论先行

| 目标 | 现状 | 本方案 |
|---|---|---|
| 让第三方能调一个已装连接器 | ⚠️ 必须在 `[connector_proxy].allow` 里**逐工具**写下来 | **改成默认全放 + 可选减法**（`allow` 收窄、`deny` 排除） |
| 让第三方知道工具**怎么调** | ❌ schema 在宿主手里，映射到 wire 时被丢掉 | `ConnectorTool.input_schema`（上游 `inputSchema` 逐字） |
| `enabled`（宿主的连接器启停） | 保留 | **不动** |
| `[connector_proxy]` 表缺席 = 代理关 | 保留 | **不动** |
| SDK 方法 `connectors.test()` | 与 wire `connector/test` 一致 | **不改名**（见 §6） |

**一句话**：把授权单位从「工具名」上移到「连接器」，把签名补上——而这两件事其实是同一件
（§3 第 1 条）。

---

## 2. 边界（非目标）

- **不改 `enabled` 的语义**。它表达「本机的 agent 会话可以用这个连接器」，不是「第三方可以调它」。
  本方案接受二者在「默认全放」下不再分离，但**不**去改造它（见 §4.5）。
- **不动引擎**（`crates/agent/`）。`[tools]` 的 `enabled`/`disabled` 与 MCP 工具的 `deferred`
  语义一行不改。
- **不新增方法、不新增路由、不新增错误码**。`deny` 复用 `policy_denied`。
- **不新增 DTO**，只在两个既有 DTO 上加字段。
- **不做** `connector/export`（连接与凭据永不跨界，`24` §5 的既有决定）。
- **不给结果脱敏**（`24` §9.1 第 5 条：半脱敏的载荷比经审计的原样载荷更危险）。

---

## 3. 设计依据（既有事实，附证据位置）

| 事实 | 位置 |
|---|---|
| **同一个宿主里，本机模型能拿到 MCP 的完整签名，第三方 SDK 永远拿不到**：MCP 工具进 registry 时 `deferred = true`，模型先只见名字桩，主动 `ToolSearch` 后拿到完整 schema | `20-tool-injection-policy.zh.md`（「工具已进 registry…`deferred = true`」及其 `ToolSearch` 行：MCP 默认 deferred、无它则 Connector schema 不可见） |
| 工具**名字**早已上 wire；被丢掉的只有 schema | `nomifun-app-server/src/catalog.rs` 的 `ConnectorTool` 等价物 `AppServerConnectorTool` 只有 `name` + `description`，注释原文「upstream tool structures and schemas stay internal」（`nomifun-api-types/src/app_server.rs:169-176`） |
| 上游确实给了 schema，且宿主**已经**把它解析并落库 | `nomifun-mcp/src/connection_test/protocol.rs:73-79`（`inputSchema` → `input_schema`）；`nomifun-mcp/src/service.rs:247-254`（探针结果连 tools 一起 `update_tools` 落库） |
| 连接器**注册**路径默认关，但**导入**路径会自动开 | `nomifun-mcp/src/service.rs:64-71`（`add_server` 硬编码 `false`）；`ui/src/renderer/pages/settings/ToolsSettings/mcpImportUtils.ts:144`（`enabled: enabled && configFields.length === 0`，默认 `true`）+ `ui/src/renderer/hooks/mcp/useMcpServerCRUD.ts:76`（导入后再打一次 toggle 落库） |
| 授权表**不在** `config/get` 投影里、也不在 `config/set` 白名单里 | `nomifun-app-server/src/agent_store.rs:142-161` |
| 引擎的条目匹配语义：**非 `mcp__` 条目精确且区分大小写；只有 `mcp__` 条目是 glob** | `crates/agent/nomi-tools/src/registry.rs:288-296`（用 `glob::Pattern`），用例表在 `:2563-2587` |
| `glob` 已是**工作区依赖**（`glob = "0.3"`），但 `nomifun-app-server` 尚未引用 | 根 `Cargo.toml:212` |
| `[tools]` 的口径是「`disabled` 在 `enabled` **之后**应用」 | `nomifun-api-types/src/tool_policy.rs:74-89` |
| app-server 的探针结果有**自己的镜像 DTO**，不与 `/api/mcp/servers/*` 共用 | `nomifun-api-types/src/app_server.rs:230`（`AppServerConnectorProbeResult`）+ `nomifun-app/src/app_server_catalog.rs:418`（`probe_result()`）；对比共用的 `McpConnectionTestResult`（`nomifun-api-types/src/mcp.rs:220`） |
| 方法计数被测试钉死且与站点同步 | `web/packages/client/src/http-transport.test.ts:257`（`DOCUMENTED_ROUTE_SPLIT = { mapped: 48, unmapped: 23 }`），门禁 `scripts/check-agent-store-release-sync.mjs` |
| 指纹是 `fp-<n>` 计数器，当前 `fp-1` | `nomifun-app-server/src/lib.rs:128`（权威）、`web/packages/protocol/src/protocol.ts:33`；门禁 `scripts/check-protocol-fingerprint.mjs` |

### 3.1 为什么这两件事其实是一件

`allow` 的逐工具形态之所以让人难受，根因是**签名不可见**：`allow = ["github__create_issue"]`
是在**看不到参数**的前提下签的字——操作者同意的是一个他无法检查的名字。把签名补上之后，
「我信任这个连接器」就成了一个**知情**的决定，逐工具列举随之失去存在理由。所以
§4 取消的是**授权粒度**，§5 补上的是让这个决定成立所必需的信息。

---

## 4. 授权形状

### 4.1 配置

```toml
[connector_proxy]
enabled = true                      # 保留：缺席或 false = 整个代理关（不变）
allow = ["mcp__github__*"]          # 可选收窄；**缺席 = 全放**
deny  = ["mcp__*__delete_*"]        # 可选减法；在 allow 之后应用
```

- **`allow` 缺席（键不存在）= 全放**；**`allow` 存在 = 收窄到命中的那些**。
  二者的区分靠 `Option<Vec<String>>`（字段类型不变），所以 **`allow = []` 仍然是「到空」= 全关**——
  这是显式声明「我列了，什么都没列」的读数，与旧语义逐字一致。
- **`deny` 是新键**，默认空 = 不减任何东西。
- 只有 `enabled = true` 且**完全没写 `allow`** 的那种配置会发生行为变化：它此前是纯 no-op
  （什么都调不了），此后是全放。构造策略时打一条 `tracing::info!` 说明这一状态，不刷 warn
  （这是本方案**想要**的语义，不是异常）。

### 4.2 判定顺序

顺序即求值顺序，照 `[tools]` 的「减法在后」口径：

| # | 门 | 不过时的码 |
|---|---|---|
| 1 | 表缺席 / `enabled != true` | `policy_denied` |
| 2 | 连接器行不存在 | `not_found` |
| 3 | `allow` **存在**且两个候选名都没命中任何条目 | `policy_denied` |
| 4 | `deny` 命中任一候选名 | `policy_denied`（措辞与第 3 条分开） |
| 5 | 连接器 `enabled == false` | `connector_unavailable` |

第 3、4 步的 reason 必须可区分：「不在 allow 里」与「被 deny 明确排除」是两个不同的处置动作。

### 4.3 词汇与匹配

两个键**统一使用引擎的 `mcp__<server>__<tool>` 词汇**（与 `[tools].enabled/disabled` 同一套），
`<server>` 可写连接器的**注册名**或 **id**：

- 匹配实现就是引擎的那 8 行（`nomi-tools/src/registry.rs:288`）：**非 `mcp__` 条目精确、区分
  大小写；`mcp__` 条目走 `glob::Pattern`**。本方案在 `nomifun-app-server` 侧加 `glob.workspace = true`
  并复制这 8 行——**匹配器是同一个 crate**，漂移风险只剩那 8 行包装，故把引擎
  `registry.rs:2563-2587` 的用例表**原样照抄**成后端测试（含 `mcp__github__*` 命中、`mcp__*` 命中、
  裸 `*` 不命中、`mcp__github` 永不命中、`mcp__[` 不 panic）。
- 每个条目对**两个候选名**求值：`mcp__<id>__<tool>` 与 `mcp__<name>__<tool>`。理由与旧 allowlist
  同时接受 id 与名字相同（`24` §9.1 第 2 条）：MCP server 按**名字** upsert，后来安装的同名者
  会接管名字并因此继承授权；id 是那条会存活下来的 pin。

### 4.4 迁移效应

| 旧配置 | 新行为 | 方向 |
|---|---|---|
| 无 `[connector_proxy]` | 代理关 | 不变 |
| `enabled = false` | 代理关 | 不变 |
| `enabled = true`，无 `allow` 键 | **全放**（此前：什么都不放） | **放宽**（本方案的目标，且有 info 日志） |
| `enabled = true`，`allow = []` | 全关 | 不变 |
| `enabled = true`，`allow = ["github__create_issue"]`（旧格式） | 新词汇下**没有条目命中** → 全关 | **收窄**（fail-closed；需改写成 `mcp__github__create_issue`） |
| `enabled = true`，`allow = ["mcp__github__*"]` | 只放该连接器 | 新语义 |

**唯一非 fail-closed 的一行是第 3 行**，且它正是本方案要的语义；第 5 行会让旧配置的集成
开始收到 `policy_denied`，这是**有意的收窄**，必须在正文与 changelog 里点名（不是 bug）。

### 4.5 代价（不粉饰）

1. **blast radius 从「钉住的名字集合」变成「该连接器此刻与将来的工具面」**。MCP 的 `tools/list`
   是**动态**的：连接器升级后新增一个 `delete_repo`，在逐工具名单下不可调，在连接器级授权下
   **自动可调**。缓释只有两条：`deny`（`mcp__<连接器>__delete_*` 这类模式可以提前钉住）与
   连接器粒度本身（只授权你信任的连接器）。
2. **审计语义变化**：从「这个工具被批准过吗」变成「发生过」（审计行仍然记连接器/工具/结果/
   字节数/耗时，不记 arguments）。
3. **`enabled` 不再是够用的唯一闸门，但它会被自动置位**：连接器进宿主有两条路，人工 REST
   默认 `false`，而 UI 的「导入本机 agent 的 MCP 配置」会自动 `enabled = true`
   （`mcpImportUtils.ts:144` + `useMcpServerCRUD.ts:76`）。也就是说在「默认全放」下
   **导入即授权第三方**。这是本方案最实的一条代价，写进正文而不是只留在代码注释里；
   可审计的抓手是 `connector/list` 已带的 `enabled` 与 `deny`。

---

## 5. 工具签名读面

### 5.1 载体：`ConnectorTool.input_schema`（不新增方法）

```text
ConnectorTool {
  name,                 # 命名空间后的公开名（已有）
  description,          # 上游工具描述逐字（已有）
  input_schema,         # 新增：上游 tools/list 的 inputSchema，**逐字**
}
```

- 它一次修好**两条**既有读取路径：`connector/get`（自上次探针的缓存）与 `connector/test`
  （现场探针，且服务端落库，顺手刷新 get）。
- **不新增方法、不新增路由** → `DOCUMENTED_ROUTE_SPLIT` 的 `48 / 23` **不变**，站点的方法计数
  不用跟着动。
- 取数路径**零新增**：`McpToolResponse.input_schema` 早已存在并已落库，缺的只是映射。
- **只碰 app-server 的镜像 DTO**：`AppServerConnectorProbeResult` 是 app-server 自己的形状
  （`probe_result()` 从 `McpConnectionTestResult` 构造），所以共用的 `McpConnectionTestResult`
  / `McpToolResponse`（`/api/mcp/servers/*` REST 与 UI 在用）**一行不改**。

### 5.2 体积

`ConnectorDetail` 今天**没有任何上限**（只有 `connector/call` 的结果有 1 MiB 上限），而 schema
是任意 JSON Schema——一个 500 工具的 server 可以吐几 MiB。规则：

- 新增 `MAX_CONNECTOR_TOOLS_BYTES = 1 MiB`（`nomifun-app-server/src/catalog.rs`，与
  `MAX_CONNECTOR_CALL_RESULT_BYTES` 同址、同族但**独立**常量）。
- 投影时按 tools 的既有顺序累加：**`name` / `description` 永远保留**（它们是「选哪个」的依据，
  且便宜）；**`input_schema` 超预算的部分整份省略**，并置 `tools_truncated: true`。
- **绝不截半个 JSON Schema**：要么整份给，要么整份不给。半截 schema 会被调用方解析并相信。
- 两个带 tools 的 DTO 各加一个 `tools_truncated`：`ConnectorDetail` 与 `AppServerConnectorProbeResult`。
- 为什么不是「整个调用回 `response_too_large`」：仓里「拒绝优于静默截断」针对的是调用方会
  parse 并相信的**载荷**；这里 names/descriptions 没有被截断，缺的是**显式标记的缺席**，不是同
  一种情形，而把一个超大连接器变成「目录完全读不出来」是更糟的失败。

### 5.3 读面不加门

`connector/get` / `connector/test` 走 `ConnectorCatalogProvider`，与 `ConnectorCallProvider`
是两个 seam，`connector_calls` 能力位只管 `call`。**不给读面加 `[connector_proxy]` 门**：

1. 工具**名字**今天已经上 wire，schema 是同一能力的更高分辨率，**不是新面**；
2. 加门会破坏既有 catalog UI（它依赖 `connector/get` 渲染连接器详情）。

### 5.4 新鲜度（登记，不在本次实现）

`connector/get` 的 tools 来自**上次探针落库**的结果，可能很旧、也可能是空（首次探针成功前
恒为空数组）。要新鲜就自己先 `connector/test`——零改动。给 `ConnectorDetail` 加
`tools_updated_at` 是**可选**改进，今天没有需求指向它，按 `web/AGENTS.md` §2 不做。

---

## 6. 命名决定：`connectors.test()` 不改名

**决定：不改。**（若将来要改，见下面「改到底」的代价。）

理由三条：

1. **这个仓库是故意把两个词分开用的：`test` = 动作，`probe` = 产物。** 动作侧是 wire
   `connector/test`、路由 `/test`、`connector_test_route`/`connector_test_impl`、
   `McpConnectionTestService::test_connection`、模块 `connection_test`；产物侧是 wire 类型
   `ConnectorProbeResult`、Rust `AppServerConnectorProbeResult`、辅助函数 `probe_status()` /
   `probe_result()`。改成 `probe()` 会得到 `probe()` → `ProbeResult`（动词名词同词，信息更少），
   而 wire / 路由 / Rust / 探测器服务**全都还叫 test**。
2. **SDK 的既有约定是「方法名 = wire 方法最后一段」**：`list` / `get` / `status` / `test` /
   `call` + 驼峰的 `authStatus` / `authStart` / `logout` 全部 1:1。只改客户端会制造第一个例外，
   而且就在 `protocol.ts` 的 `ConnectorProbeResult` 旁边。
3. **「名字不够准」这个感觉是对的，但换 `probe` 治不了**：`test()` 真的会建立连接（stdio 会
   **spawn 子进程**，http/sse 走 `initialize` → `notifications/initialized` → `tools/list`）
   并且**结果落库**——`test` 与 `probe` 都没表达出后者。该动的是**文档注释**，不是标识符；
   本次因为 schema 经它带出，注释本来就要重写。

**被搁置的备选（登记）**：若要改名，应**连 wire 一起改**（`connector/probe` + 路由 `/probe`），
落点为本仓代码 4 处 + 夹具 3 处（`mock-server.ts` / `smoke.ts` / `sdk-live-oauth.ts`）+
正文 5 处 + 站点 4 处。**本次指纹无论如何都要 bump，且握手是严格相等**，所以旧客户端本来就必须
升级——若真要改，现在的兼容性代价是零。记录在此，不在本次做。

---

## 7. 指纹与跨仓同步

按 `web/AGENTS.md` §5 四步：

1. **指纹 `fp-1` → `fp-2`**（现有 DTO 加字段属于触发分支）。落点由
   `bun run check:fingerprint` 点名：权威 `nomifun-app-server/src/lib.rs:128`，镜像
   `web/packages/protocol/src/protocol.ts:33`、`web/packages/client/src/http-transport.ts:43`、
   `web/scripts/mock-server.ts:20`、`web/scripts/smoke.ts`（2 处）、
   `web/packages/sdk/src/readiness.test.ts`（2 处），站点仓 `typescript-sdk.md` 中英各 1 处。
2. **正文**：`05`（头部指纹 + §4.3 连接器读面 + §4.3.2 三道门重写）、`10-public-contracts.md`
   （若它登记了 `connector` 的字段/方法）、`24` §5.1/§9.1（原判断**就地标注为已改**，保留原
   理由以便对照——该文档既有的做法）、`25`（指纹现值）、`16` §7 决策 4 的**台账追加一行**、
   `README.md` 本轮记录、本文 §9.1。
3. **站点仓**（`C:\workspace\agent-store-site`）：`typescript-sdk.md` 中英的指纹示例、
   `ConnectorTool` 形状、`[connector_proxy]` 说明；`changelog` §4 未发布台账。
   **方法计数不变**（没新增方法），故 `check:release-sync` 的计数半边不动。
4. **完成标准**：旧值 `fp-1` 在本仓代码里归零（只剩历史散文）；
   `cargo test -p nomifun-app-server` + `cargo test -p nomifun-app` 绿；
   `cd web && bun run typecheck && bun run test` 绿；`bun run check` 绿
   （指纹 + release-sync + market manifest …）。

---

## 8. 风险与失败模式

| 风险 | 处置 |
|---|---|
| 旧 allowlist 条目在新词汇下失配 → 集成开始收 `policy_denied` | **有意的 fail-closed 收窄**，在 `05`/changelog/本文 §4.4 点名；恢复只需改写条目 |
| `enabled = true` + 无 `allow` 的旧配置从「全关」变「全放」 | 这是本方案的目标语义；构造策略时打 `tracing::info!` 说明当前是「无 allow/deny，全部可调」 |
| 连接器升级后新增危险工具自动可调 | 登记为已知代价（§4.5 第 1 条）；`deny` 是提前钉住的抓手；`connector/list` 的 `enabled` 是可审计面 |
| 超大 schema 把 `connector/get` 撑爆 | `MAX_CONNECTOR_TOOLS_BYTES` + `tools_truncated` 自曝（§5.2） |
| 后端复制引擎的匹配规则后漂移 | 用同一个 `glob` crate + 照抄引擎用例表（§4.3） |
| 给读面顺手加代理门，弄坏既有 catalog UI | 明写「读面不加门」及其两条理由（§5.3） |

---

## 9. 实施顺序与验收

```text
 1. DTO 加字段（input_schema + 两个 tools_truncated）        → cargo check -p nomifun-api-types
 2. 授权策略（allow 缺席=全放 / deny 减法 / glob 匹配）      → cargo test -p nomifun-app-server
 3. 体积预算常量 + 投影（get 与 probe 两处）                 → 单测：逐字相等 / 超限置 flag
 4. 口径注释（connector_call 的三道门、capabilities）        → 重读核验，无悬空旧说法
 5. TS（protocol.ts 三字段 + connectors.ts 注释）            → cd web && bun run typecheck
 6. 指纹 fp-1 → fp-2（本仓 6 文件 + 站点 2 处）               → bun run check:fingerprint
 7. 正文（05 / 10 / 16 台账 / 24 / 25 / README）              → 旧值归零
 8. 站点仓（typescript-sdk 中英 + changelog）                 → check:docs-sync 0 drift
 9. 全量验证                                                  → cargo test ×2 + web test + bun run check
```

> **第一道真实性闸门**是第 3 步的 e2e：它把「schema 真的逐字来自上游 `inputSchema`」从
> **读代码结论**变成**实测证据**（不是「字段有值」就算过）。**已落地**（见 §9.1 的 e2e 行）：
> 真服务端 + 真组合根，且刻意断言「服务器没声明 schema 的工具不凭空长出一个」——只断言
> 「有值」的话，一个把空 schema 补成 `{"type":"object"}` 的实现也能过。

### 9.1 落地记录

**已落地（2026-09-22）。逐层改动与真实读数：**

| 层 | 实际改动 | 验证读数 |
|---|---|---|
| DTO | `nomifun-api-types/src/app_server.rs`：`AppServerConnectorTool.input_schema`；`AppServerConnectorDetail.tools_truncated`；`AppServerConnectorProbeResult.tools_truncated`（三者 additive，bool 用既有的 `#[serde(default)]` 口径，与 `AppServerSkillFileList.truncated` 同形） | `cargo check -p nomifun-app` 通过 |
| 授权策略 | `nomifun-app-server/src/agent_store.rs`：`AgentStoreConnectorProxy` 加 `deny`；`ConnectorProxyPolicy.allow` 改 `Option<HashSet>`、新增 `deny: HashSet`；`decide` 五步；私有 `tool_name_matches`（与引擎同一个 `glob` crate，只包了 8 行）；`warnings()`（纯函数，照 `NomiToolPolicy::syntax_warnings` 的先例） | `cargo test -p nomifun-app-server --lib` → **145 passed / 0 failed**（含 9 条策略用例：全放 / `allow=[]` 全关 / allow 命中 / deny 优先且 reason 可区分 / glob 整连接器 / 表缺席仍全关 / 未启用仍全关 / 引擎用例表照抄 / 迁移与配置告警） |
| 依赖 | `nomifun-app-server/Cargo.toml` 加 `glob.workspace = true`（工作区已有 `glob = "0.3"`） | `Cargo.lock` +1 行 |
| 体积预算 | `nomifun-app-server/src/catalog.rs` 加 `MAX_CONNECTOR_TOOLS_BYTES = 1 MiB` 并从 `lib.rs` 再导出 | 同上 |
| 投影 | `nomifun-app/src/app_server_catalog.rs`：新增 `connector_tools(Option<Vec<McpToolResponse>>) -> (Option<Vec<AppServerConnectorTool>>, bool)`，`get` 与 `probe_result` 共用（原先是两处各写一遍的 `map`） | `cargo test -p nomifun-app --lib connector_tools` → **4 passed / 0 failed**（逐字相等 / `None` 保持 `None` / 超限整份省略且 flag 为真 / 预算按序消耗） |
| 接线与告警 | `nomifun-app/src/services.rs`：`resolve_connector_proxy_policy` 与字段注释改口径；启用后除既有 info 外逐条 `tracing::warn!` 输出 `warnings()`（「开了代理却没写名单」「条目不是 `mcp__` 形状」） | 随 `cargo check` 通过 |
| 口径注释 | `app_server_connector_call.rs` 的「三道门」第 2 门与 Gate 2 注释、`catalog.rs` 的 `ConnectorCallProvider` 两条门、`protocol.ts` 的 `connector_calls` 能力位注释、`services.rs` 的字段与解析函数注释、`05` §4.3 的能力位段 | 重读核验：**代码与规格正文已无「显式 allowlist / `allow` 为空即默认拒绝」的旧说法**；`README` / `24` §5.1 / `05` 头部日期块里保留的旧措辞是**历史记录**（描述当时的判断），各自带修订指引 |
| TS | `protocol.ts`：指纹 `fp-1` → `fp-2` + 三个字段；`connectors.ts`：`test()` / `call()` 文档注释重写（含「为什么不叫 `probe`」）；`store.test.ts` 三处夹具补 `tools_truncated` | `cd web && bun run typecheck` → **exit 0**；`bun run test` → **495 passed / 1 skipped**（65 文件） |
| 指纹 | 本仓 7 文件 10 处 + 站点 2 处 | `bun run check:fingerprint` → `✓ "fp-2" … 10 landing point(s) in 7 file(s) here and 2 file(s) in the docs site`。**其中一处是本轮补上的门禁盲区**：`scripts/probe-agent-store-runtime.mjs` 此前停在 `2026-09-14`（跨了两次形状都没被发现，因为没有东西指向它），本轮改为 `fp-2` 并**列入 `MIRRORS`** |
| 夹具与 smoke | `mock-server.ts`：`MockConnector.tools` 允许 `input_schema`、两个工具各带一份真 schema、`connector/get` 与 `connector/test` 补 `tools_truncated`；`smoke.ts` 新增一条断言（探针必须带回参数且 `tools_truncated === false`） | `bun scripts/smoke.ts`（mock 宿主，端口 17990）→ `✓ connector/test carries tool parameters verbatim (browser_navigate)`，整体 **smoke passed** |
| **e2e（§9 的「第一道真实性闸门」）** | 新增 `crates/backend/nomifun-app/tests/connector_tools_e2e.rs`（+ `Cargo.toml` 的 `[[test]]`）：真组合根 `create_router` + **真 MCP server**（跨平台 stdio 夹具，由宿主自己 spawn）→ 注册 → app-server 握手 → `POST /api/app-server/connectors/{id}/test`，断言 ① `input_schema` **与夹具声明的 schema 逐字相等**、② 服务器没声明 schema 的工具**不会凭空长出一个**、③ `tools_truncated === false`、④ 该读面在连接器**未启用**时也工作（它不该被调用代理收窄）。为此外加 `fake_stdio_mcp.mjs` 的 `tools/list`——它此前只答 `initialize` 与 `tools/call`，**根本没法做成功探针** | `cargo test -p nomifun-app --test connector_tools_e2e` → **1 passed / 0 failed**（3.5s）；夹具既有使用者 `cargo test -p nomifun-mcp --test connection_test_integration` → **25 passed / 0 failed**（加 `tools/list` 不影响任何既有断言）；登记门禁 `every_top_level_integration_test_is_registered` → **1 passed / 0 failed** |
| 正文 | `05`（头部指纹 + §4.3.2 重写 + 新增 §4.3.3）、`10` §7、`16` §7 决策 4 台账、`24` §5.1 与 §9.1（原判断就地标注为已修订）、`25` §2、`README` 本轮 | 旧值 `fp-1` 在本仓代码里归零 |
| 站点仓 | `typescript-sdk.md` 中英（指纹、`test()` 注释、`call()` 段落新增「参数怎么知道」）、`changelog.md` §4、`upgrade.md` §8（**改为「工作区领先于已发布产物」**并点名未发布增量） | `bun run check:docs-sync` → **10 page(s) / 0 drift**；`bun run test:docs-sync` → **16 pass / 0 fail** |

**与方案的偏差**：

1. **`deny` 的告警与 `warnings()` 是方案外新增的**（§4.4 只写了「打一条 info」）。落地时发现真正会咬人的是**迁移陷阱**：旧写法 `<连接器>__<工具>`
   在新词汇下永远不命中（所有候选名都以 `mcp__` 开头），而那种配置**看起来**是在授权。于是把它做成可测的纯函数 `warnings()`（照
   `NomiToolPolicy::syntax_warnings` 的既有形状），在组合根逐条 warn，而不是在构造函数里打日志（后者不可测）。顺带覆盖了「开了代理却没写任何名单」。
2. **`allow` 的「缺席」与「空表」刻意不合并**（方案 §4.1 已写，实现时确认 `Option` 原样保留即可，无需额外字段）。
3. **`connector/call` 的 `deny` reason 与 `allow` 的 reason 分开措辞**（前者 `excluded by …deny`，后者 `not in …allow`），并有测试钉住——两个 `policy_denied` 的处置动作不同。

**未做（与方案一致）**：`tools_updated_at`（§5.4）、`connectors.test` → `probe` 改名（§6/§10）、HTTP/SSE 会话复用、`enabled` 与代理授权的分离（§4.5 第 3 条，明确不动）。

---

## 10. 未做 / 登记

- **HTTP / SSE 会话复用**：延续 `24` §9.1 的决定（只池化 stdio），不在本次范围。
- **`tools_updated_at`**：§5.4，无需求不做。
- **`connectors.test` → `probe` 改名**：§6，登记为「要做就连 wire 一起改」。
- **`enabled` 与代理授权的分离**：§4.5 第 3 条，本次明确**不动** `enabled`。
