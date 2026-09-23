# 连接器用户凭据（key / token 类）· 技术方案

> 状态：**设计定稿，未实施**（2026-09-23）。决策 D1–D6 见 §4，均有取值与代价。
> 前置：`02-codebuddy-workbuddy-import-spec.md`（§5/§10 `userConfig → CredentialSchema`、值不入库）、
> `05-flowy-agent-store-app-server-protocol.md`（协议正文）、`06-connector-oauth-security.md`（并行的 OAuth 通道）、
> `20-tool-injection-policy.zh.md`、`21-open-decisions.zh.md`（D5=C 及 §116 落地记录）、
> `24-external-agent-skill-and-mcp-access.zh.md`（`connector/call`）、
> `16-sdk-webui-site-priority-plan.zh.md`（R22 `[credentials]` 的由来）、`web/AGENTS.md` §5（指纹与跨仓同步）。
> 用途：回答三件事——**(1)** 需要用户输入 key / token 的连接器，凭据从哪来、存在哪、何时注入；
> **(2)** 这条链路上 host / 协议 / WebUI / SDK 各自负责什么；**(3)** 本方案要修订 D5=C 的哪一条边界。

---

## 1. 结论先行

| 目标 | 现状 | 本方案 |
|---|---|---|
| 让"要用户输入 key"的连接器可配置 | ❌ 无输入口；市场声明在导入期被丢弃 | 声明归一为 `CredentialSchema`，由协议下发，WebUI 用同一份 schema 渲染表单 |
| 凭据怎么被用上 | ⚠️ stdio `env` 解析引用；**headers 只在 agent 装配路径解析**；url 不解析 | 引用升级为模板，**headers / env / url 三处统一解析**，单一入口 |
| 凭据存哪 | ✅ `~/.agent-store/config.toml [credentials]`（进程 env 兜底） | 沿用；**secret 与 plain 分流**（§5.3） |
| 凭据值是否上协议 | **刻意不上**（`21` D5=C：`config/get` 投影不含、`config/set` 白名单拒绝） | **有限放开**：只放行写入与键名元数据，值永不回传 |
| 连接器认证方式如何表达 | ⚠️ UI 的 `auth_mode` 从 transport 推导（http/sse → `oauth`） | 协议新增 `credential.mode`，与市场声明的认证方式对齐 |
| 市场包内可执行文件 | — | **永不执行**（§2、§8） |

**一句话**：把"需要用户输入"提升为与 OAuth **平行**的一等凭据类型，复用既有的 `secret:` 引用与
`[credentials]` 表，只新增"声明下发 + 定向写入"这一层协议，并把既有的"整值引用"升级为模板替换。

### 1.1 三条不变量

1. **值永不越界**：不进快照、不进 DB 定义行、不进日志、协议回包只含键名与"缺哪些"。
2. **占位与注入同构**：用户填写的 key 与 OAuth token 走同一个注入器（§3.2 第四节给出市场侧的既有证据）。
3. **声明只归一一次**：市场特有字段只在 host 侧解析，WebUI 与 SDK 消费同一份结构，均不含市场特有逻辑。

---

## 2. 边界（非目标）

- **不执行市场包内的任何脚本**。导入语义是 materialize + 注册为惰性组件（`script` 组件本就如此建模）。
  依据见 §3.3 第二条。若将来需要执行，须另行立项，并带显式开关、签名与门禁。
- **不做跨连接器取凭据**（市场的 `from_connector`）。凭据依赖关系会成图，需要额外的一致性语义；
  §6.1 的注入规则仍按 `{header, value_template, source}` 建模以留位。
- **不做运行环境 / 套餐门禁**（市场的 `when.active_in` 中的 `plan:*`）。host 无套餐概念，明确丢弃并登记。
- **v1 只覆盖 `type: mcp` 的连接器**（市场 283 个里 215 个）。`type: cli`（44 个）另立一轮。
- **不把 `[credentials]` 变成可读面**。只放开"写入 + 键名元数据"，值侧维持不可读。
- **不新增 OAuth 能力**。OAuth 三方法与其状态机不动；scope 挑战（`06` 之外的部分）不在本方案内。

---

## 3. 设计依据（既有事实，附证据位置）

### 3.1 本仓已有（不新建机制）

| 事实 | 位置 |
|---|---|
| 引用语法为 `secret:<NAME>`，全仓唯一（刻意与浏览器引擎同一字面量） | `nomifun-common/src/secret_ref.rs`（`SECRET_PREFIX` / `parse_secret_ref`） |
| 值存放为 `~/.agent-store/config.toml` 的 `[credentials]`，进程 env 兜底（config 优先） | `16-sdk-webui-site-priority-plan.zh.md` §682、`21-open-decisions.zh.md` §116 |
| 宿主启动时把该表装进进程 | `apps/agent-store/src/main.rs:333`（`secret_ref::set_credentials`） |
| 解析口径：普通值原样透传；引用查表；**解析不到即省略该变量**（fail-closed，绝不注入字面量）；`missing` 只回报键名 | `nomifun-common/src/secret_ref.rs`（`resolve_env`） |
| stdio `env` 在探针与调用两条路都解析 | `nomifun-mcp/src/connection_test/mod.rs:460`、`connection_test/session.rs:49` |
| **headers 只在 agent 装配路径解析** | `nomifun-ai-agent/src/factory/nomi.rs:2178`（`resolve_env(headers)`）；`:2199` 另有"形似引用但不是引用"的告警分支 |
| 导入侧已把 `userConfig` 映射为 `CredentialSchema` 组件（`{key,type,required,sensitive}`），值一律 `[REDACTED]`、永不入库 | `nomifun-importer/src/import.rs:718+`；`02` §10 / TC-IMP-009 |
| 敏感判定谓词已存在，MCP `env` 与 `userConfig` 共用 | `21-open-decisions.zh.md` §116（`is_sensitive_field`） |
| 只读面已确立"只过 key 名"的原则 | `nomifun-api-types/src/app_server.rs:1266` |
| 值侧已有正确姿态的先例可照抄：值单向写入、加密进机器绑定 vault、永不回传、列表只给元数据 | `nomifun-api-types/src/secret.rs` |
| 凭据表**刻意**不在协议读面与写面（有测试守着） | `nomifun-app-server/src/agent_store.rs:2085-2098`（"Hand-edited, never on the wire"）；`16` §688；`21` D5=C |
| 连接器 OAuth 面的三个方法（本方案平行的对象） | `nomifun-app-server/src/lib.rs:1299-1310` |

### 3.2 市场侧：声明是三件套（283 个连接器实测）

| 层 | 作用 | 关键字段 |
|---|---|---|
| `.codebuddy-connector/connectors.json`（508 KB / 283 条） | 声明该连接器**是否需要用户填** | `auth_mode`、`type`、`visible_in`、`minWorkbuddyVersion`、`examples_zh/_en`、`name_zh/_en`、`description_zh/_en`、`source`；顶层另有 `auth_injection_rules` |
| `connectors/<slug>/mcp.json`（239 个） | 传输**模板** | `url`、`type`、`headers`、`staticHeaders`、`timeout` |
| `connectors/<slug>/token-schema.json`（61 个） | **表单声明**（标题、说明、取密钥入口、字段清单） | 见 §5.2 |

**一对一关联**：索引中 `auth_mode = "token"` 恰为 **61** 条，`token-schema.json` 恰为 **61** 个，
且 61/61 全部存在（缺 0）。语义为：索引声明"需要填"，同目录 schema 声明"填什么、怎么填、去哪取"。

分布：

- `auth_mode`：空 204 / `token` 61 / `server-side` 12 / `oauth` 4 / `mcp` 1 / `oneid-token` 1。
- `type`：`mcp` 215 / `cli` 44 / 空 24。
- `visible_in`：`internal` 150 / `iOA` 149 / `selfhosted` 148 / `cloudhosted` 147 / `plan:ultimate` 1 / `plan:exclusive` 1。

**占位符：四类落点、两种形式，共 74 处**

| 落点 | 形式 | 例 | 计数 |
|---|---|---|---|
| headers | 整值 | `X-Api-Key: "${AGENT_EARTH_API_KEY}"` | 整值合计 **27** |
| headers | 内嵌 | `Authorization: "Bearer ${API_KEY}"` | 内嵌合计 **47** |
| env | 整值 | `DCS_PAT: "${DCS_PAT}"` | env **5** |
| url query | 内嵌 | `…/mcp?token=${GILDATA_TOKEN}` | url **10** |

落点合计：headers 59 / env 5 / url 10。拼写**只有一种**：74/74 全为 `${NAME}`
（无 `${user_config.KEY}`、`{{VAR}}`、`$VAR` 变体）。

**既有引用契约的覆盖缺口**：`parse_secret_ref` 要求值为**恰好** `secret:NAME`，据此只能表达
**27/74**（整值部分）；其余 47 处为内嵌形式，须由模板替换表达（§6.2）。

**`auth_injection_rules`（索引顶层，2 条：tencent-docs / ima-mcp 的双 token 场景）**

```jsonc
{ "id": "tencent-docs-dual-token",
  "when": { "active_in": ["iOA", "plan:ultimate", "plan:exclusive"] },
  "applies_to_connectors": ["tencent-docs", "tencent-docs-oa"],
  "timing": "request",
  "inject": [
    { "from_connector": "tencent-docs",    "token_type": "mcp-oauth",
      "header": "Authorization",        "value_template": "Bearer ${access_token}" },
    { "from_connector": "tencent-docs-oa", "token_type": "oneid-token",
      "header": "X-Oneid-Access-Token", "value_template": "${access_token}" } ] }
```

三条对设计的约束：

1. 注入使用**与 `mcp.json` 相同的模板语法**，即市场内部"用户填的 key"与"某连接器的 OAuth token"
   本就同构，本方案的不变量 2 与之一致。
2. **`Bearer` 前缀写在模板里**（有前缀 / 无前缀两种形态皆存在），因此归一化必须原样保留模板文本，
   不得按 header 名自动补前缀。
3. `from_connector` 与 `when.active_in` 是本仓模型之外的维度，处置见 §2。

### 3.3 源数据的两个缺陷（影响 §2 与 §8）

1. **密钥作为默认值随包分发**：`cisp-mcp` 的 `CISP_API_KEY`（`type: password`、`required: true`）
   带一个 `sk-agw-…`（56 字符）的 `defaultValue`。全市场仅此 1 处（按前缀全目录检索命中 1 个文件；
   61 个 schema 中再无密钥形状的默认值）。未验证其可用性——不对第三方凭据做验证性调用。
   归属：**源侧行为，不在本方案处置范围内**。对本仓的要求只有一条——该默认值不得进入我们的任何
   产物（§9 第 2 步的导入期丢弃 + 告警，§8 第 1 条的门禁防回归）。
2. **可执行载荷与远程下载随包分发**：`shanlong-claw` 含 `install.ps1`（25 KB）、`install.sh`（18 KB）、
   `launch.cjs`、`launch.ps1`、`launch.sh`、`connector-readonly-policy.js`、
   `verify-connector-package-scope.js`，并有 `install-url.conf` 指向
   `https://chat-cdn.tcsl.com.cn/slclaw/workbuddy-cli/`（含 `.version`、`.manifest.json`）。
   同类还有 `cli/`（3 个连接器）、`scripts/`（3 个）、`woscli` 的安装脚本、`welife-…/client.cjs`。
   处置：§2 第一条（永不执行）。

### 3.4 两处实缺（当前会导致"已配置却发不出去"）

| 缺陷 | 证据 | 影响 |
|---|---|---|
| 探针 / 调用路径不解析 headers 引用 | `nomifun-mcp/src/connection_test/mod.rs:622-652`（`request_headers()` 只 `build_http_headers` 后判断 `Authorization`，无 `resolve_env`） | `headers: { "x-api-key": "secret:X" }` 的连接器，`connector/test` 与 `connector/call` 会把 **`secret:X` 字面量**当作密钥发出；同一"引用"在 stdio 与 http/sse 上行为不一致 |
| url 无解析点 | 本方案不涉及处无解析代码；市场 10 个连接器把凭据放入 url query | 含凭据的 URL 直接以字面量发出 |
| 传输拼写未归一 | 市场 `streamableHttp` 193 + `streamable-http` 7（合计 84%）；`nomifun-mcp/src/types.rs:67-104` 的 `from_db` 只接受 `stdio|sse|http`，其余返回 `InvalidTransport`；适配器路径宽容（`nomifun-mcp/src/adapters/codebuddy.rs:127-128` 非 `sse` 一律 `http`） | 归一化必须在导入期完成，否则出现"装得进、读不回" |

---

## 4. 决策（D1–D6）

| # | 决策 | 取值 | 理由 | 代价 |
|---|---|---|---|---|
| D1 | 值的存储粒度 | **per-principal**：最小步命名空间化 `<principal>:NAME`，终态为 per-principal 表 | `[credentials]` 现为宿主级扁平映射，`oauth_tokens` 亦按 `server_url` 唯一并在冲突时覆盖（`nomifun-db/migrations/001_v3_baseline.sql:1367`、`repository/sqlite_oauth_token.rs:38-45`），故共享宿主上多用户互相可见 | 需要迁移与 key 迁移逻辑；见 §7 |
| D2 | secret 与 plain 的落点 | **secret → 凭据库**（单向写）；**plain → 连接器 transport config** | plain 值（`HOST`/`PORT`/`ENV`/`SCHEMA`）本非秘密，可见/可导出符合直觉；凭据库保持最小 | transport config 需要容纳这些值，且它们会出现在 `connector/list` 的传输摘要之外（需确认不泄漏不需要的字段） |
| D3 | i18n 归属 | **host 归一**，成对下发 `{zh, en}`；回退链在 host 侧实现 | 市场文案不可能进入 SPA 字典；两个客户端必须行为一致 | host DTO 体积略增；回退链需单测覆盖 |
| D4 | 跨连接器取凭据（`from_connector`） | **v1 不支持**，注入规则按 `{header, value_template, source}` 建模留位 | 凭据依赖成图，需要一致性/生命周期语义 | 市场 2 条规则的能力暂缺 |
| D5 | 环境 / 套餐门禁（`when.active_in`、`visible_in`） | 只映射可对应者（`selfhosted` / `cloudhosted` → host 模式）；`plan:*` **丢弃并登记** | host 无套餐概念 | 与市场行为不完全对齐 |
| D6 | `type: cli`（44 个） | **v1 不覆盖** | 与 MCP 传输无关，属另一运行时面 | 市场覆盖率为 215/283 |

---

## 5. 数据模型

### 5.1 引用与模板

- 保留 `secret:<NAME>` 作为**整值引用**的既有语义（`parse_secret_ref` 不变）。
- 新增**模板形式**：`${secret:<NAME>}` 可出现在任意字符串内，解析时做子串替换。
  归一化产物形如 `Bearer ${secret:TDENGINE_API_KEY}`、`?token=${secret:GILDATA_TOKEN}`。
- 非秘密字段使用 `${NAME}`（与现有 `secret:` 命名空间区分），来源为 D2 的 plain 存储。
- 两种形式都 fail-closed：**无法解析即不发起请求**，返回缺失键名（§6.2）。

### 5.2 `CredentialSchema`（协议下发形态）

沿用既有 `KIND_CREDENTIAL` 组件，扩展字段（现状仅有 `key/type/required/sensitive`）：

| 层级 | 字段 | 说明 |
|---|---|---|
| 连接器 | `title{zh,en}`、`description{zh,en}`、`doc_url{zh,en}`、`doc_label{zh,en}` | 市场 `token-schema.json` 的顶层键；`doc_url` 有 `docUrl_en` 变体（1 例），按语言分别取 |
| 字段 | `key`、`kind: secret\|plain`、`required`、`label{zh,en}`、`placeholder{zh,en}`、`description{zh,en}`、`default_value?` | `type: password` → `secret`；`type: text` 且键名命中敏感谓词 → `secret`，否则 `plain` |

i18n 回退链（host 侧实现一次）：`*_en → 中文 → key`。缺口规模：`label_en` 缺 12、`placeholder_en` 缺 16、
`description_en` 缺 14（共 74 个字段），`title_en` 缺 1、`description_en` 缺 2、`docLabel_en` 缺 15（共 61 个 schema）。

字段规模（驱动 UI 形态）：单字段 51 个、双字段 8 个、三字段 1 个、四字段 1 个；
`required: false` 仅 4 个字段。市场 61 个 schema 无结构异常（无缺 label、无重复 key、无非字符串 type）。

`tdengine` 是混合形态的基准用例：4 个字段中 3 个为 `plain`（`SCHEMA=http`、`HOST=localhost`、`PORT=6042`，
均带默认值）、1 个为 `secret`；其 `mcp.json` 的 url 与 header 同时使用模板：

```jsonc
"url": "${TDENGINE_API_SCHEMA}://${TDENGINE_API_HOST}:${TDENGINE_API_PORT}/api/v1/mcp/stream",
"headers": { "Authorization": "Bearer ${TDENGINE_API_KEY}" }
```

### 5.3 存储与键控

| 类别 | 落点 | 可见性 |
|---|---|---|
| `secret` 字段值 | `[credentials]`（键为 `<principal>:<KEY>`，见 D1） | 只能写入；任何读面只回键名与 `missing` |
| `plain` 字段值 | 连接器 transport config | 随连接器定义可见 |
| `secret` 字段的 `defaultValue` | **丢弃 + 告警** | 依据 §3.3 第一条 |

写入路径要求：原子写、文件权限 600（与 GitHub 官方 token 处理口径一致）；`[credentials]`
仍不进 `config/get`。

---

## 6. 接口

### 6.1 协议面（WebUI 与 SDK 的共同契约）

在既有 OAuth 三方法旁新增三方法，并在连接器摘要 / 详情挂统一字段：

```jsonc
credential: {
  mode: "none" | "oauth" | "token",
  status: "not_required" | "requires_input" | "configured" | "error",
  missing: ["TDENGINE_API_KEY"],            // 仅键名
  fields: [{                                // 仅元数据，永不含值
    key, kind: "secret" | "plain", required,
    label: {zh, en}, placeholder: {zh, en}, description: {zh, en},
    default_value?: string,                 // 仅 plain 携带
    doc_url: {zh, en}, doc_label: {zh, en}
  }]
}
```

| 方法 | 请求 | 响应 |
|---|---|---|
| `connector/credential/get` | `{connector_id}` | 上述 `credential` 块 |
| `connector/credential/set` | `{connector_id, values: {KEY: "…"}}` | 新的 `credential` 块（不含值） |
| `connector/credential/clear` | `{connector_id, keys?: [KEY]}` | 新的 `credential` 块 |

- **状态词表收敛**：OAuth 的 `authenticated` / `not_authenticated` 一并映射进
  `requires_input / configured / error`，UI 只实现一种三态。
- **`connector/test`** 需能单独回报"缺凭据"（typed error），而非笼统失败。
- **`auth_mode` 去重**：协议内以 `credential.mode` 表达认证方式；UI 现有那个由 transport 推导的
  `auth_mode`（http/sse → `oauth`）不得继续用于决定按钮形态，否则 61 个 `token` 类连接器会得到
  OAuth 授权入口。
- 注入规则建模为 `{header, value_template, source}`；v1 的 `source` 仅取本连接器凭据（D4）。

### 6.2 运行时解析

- `secret_ref` 由"整值引用"扩展为模板替换（`5.1`），实现为单一函数。
- **单一入口**：解析后的 transport 由一个方法产出，probe / call / agent 三条路共用，
  消除 §3.4 第一条的分叉。
- 三处落点全部覆盖：headers、env、url。
- 失败模式：缺凭据 → 不发起请求 → 返回 `missing credential: <KEY>`；日志只记键名。

### 6.3 WebUI

- 由 §5.2 的 schema 驱动单一表单组件：标题 / 说明 / 取密钥入口（带语言回退）/ N 个字段。
  `secret` 字段为掩码输入、**不预填、不回显**；`plain` 字段可预填 `default_value`。
- 入口与徽标：`mode = token && status = requires_input` → 「填入凭据」；`mode = oauth` 保留既有「授权」。
  徽标统一为三态，并显示缺失项数量。
- 第三方文案（含市场作者写的安全说明，如"仅存本机，请勿发给对话里的智能体"）随 schema 下发；
  SPA 自身 chrome 仍走既有 i18n。

### 6.4 SDK 与跨仓

- `ConnectorClient` 新增 `credentials(id)`、`setCredentials(id, values)`、`clearCredentials(id)`；
  协议 DTO 镜像 host，含 `LocalizedString`。
- 新增方法会改动 `web/packages/client/src/http-transport.test.ts` 的 `DOCUMENTED_ROUTE_SPLIT`、
  站点 `content/docs/{zh-CN,en-US}/typescript-sdk.md` 的计数与常量、`changelog`，并
  **bump `fp-8 → fp-9`** 后两仓同步（门禁：`check:fingerprint`、`check:release-sync`）。

---

## 7. 兼容与迁移

| 项 | 处置 |
|---|---|
| 传输拼写 | 导入期归一为 `http` / `sse` / `stdio`（§3.4 第三条）；`from_db` 保持严格 |
| 市场 `auth_mode` | 作为声明值导入，映射为 `credential.mode`；与 UI 推导值区分（§6.1） |
| `oauth_tokens` / `[credentials]` 键控 | D1 迁移；旧行 principal 为空者按"宿主级"处理并登记 |
| OAuth 连接器 | 行为不变；仅状态词表映射 |
| 新增 DTO 字段 | 均为可选字段，旧客户端忽略即可 |
| 指纹 | 本方案实施时为 `fp-9`；本文档本身不含 wire 变更 |

---

## 8. 风险与失败模式

1. **默认值泄密回归**（§3.3 第一条）。缓解：导入期丢弃 `secret` 字段的 `defaultValue` 并告警；
   对本仓加一条机械门禁（`secret` 类字段带 `defaultValue`，或 `defaultValue` 形状像密钥即失败），
   使该行为无法经导入进入我们的产物。
2. **`secret:` 字面量外泄**（§3.4 第一条）。缓解：单一解析入口 + 集成断言（外发 header 必须是解析后的真值，
   日志不得出现值）。过渡期内，未解析的引用宁可导致"缺凭据"错误，也不得原样发出。
3. **URL query 携带凭据**（市场 10 例）。凭据会进入代理日志、`Referer`、服务端访问日志；
   导入期单独告警，不作为推荐形态。
4. **共享宿主上的凭据互见**（D1 未完成前）。多用户场景下必须先完成 per-principal 键控。
5. **包内可执行载荷**（§3.3 第二条）。导入永不执行；文档与 UI 明确这一点。
6. **明文落库**：`oauth_tokens.access_token` 现为明文（实测 40 字符），全仓无 DB 层加解密 helper；
   本方案不新增该能力，但凭据表的最小化（D2）与不可读（§2）降低了暴露面。
7. **多字段表单的可用性**：4 字段（`tdengine`）含 plain 与 secret 混合，UI 必须区分二者
   （哪一项会被保存到凭据库、哪一项会写进连接器配置）。

---

## 9. 实施顺序与验收

| 步 | 内容 | 验收 |
|---|---|---|
| 1 | 修 §3.4 前两条：`request_headers()` 接入解析入口；url 解析；传输别名归一 | `nomifun-mcp` 单测：含引用的 headers / url 发出解析后的真值；`from_db("streamableHttp")` 不再报 `InvalidTransport`。零协议变更，可独立合入 |
| 2 | 导入层：读 `token-schema.json` + 市场 `auth_mode` → 归一为 `CredentialSchema`（§5.2）；`secret` 字段的 `defaultValue` 丢弃并告警 | 导入测试：展示字段与 i18n 回退齐全；以 §3.3 第一条为夹具，断言 `[REDACTED]`、值不进快照与 DB |
| 3 | 运行时：模板解析（§6.2）+ typed `missing credential` | 集成测试：内嵌模板被正确替换；缺凭据时不发起请求且错误只含键名；日志无值 |
| 4 | 协议 + SDK：三方法 + `credential` 块 + 状态词表 + `fp-9` + 站点同步 | `cargo test -p nomifun-app-server`；`check:fingerprint` 十处落点一致；`check:release-sync` 计数一致；站点 `check:docs-sync` 0 drift |
| 5 | WebUI：schema 驱动表单 + 三态徽标 | 组件测试（i18n 回退、`secret` 不预填）；`cd web && bun run typecheck && bun run test`；手测双字段表单 |
| 6 | 存储（D1）：per-principal 键控 | 两个 principal 互不可见；迁移测试 |
| 7 | D4 / D5 / D6 的登记项 | 各自立项 |

### 9.1 端到端验收（活体）

以 mock key-based MCP server 为对象（沿用既有 live 脚本形态）：

1. 未填凭据：`connector/test` 返回 `requires_input`，且 mock **未收到任何请求**；
2. `credential/set` 后：`connector/test` 通过并返回工具表；
3. 外发 header 为模板解析后的真值（含模板中的 `Bearer ` 前缀）；host 日志只含键名；
4. `credential/clear` 后回到 `requires_input`；
5. 覆盖双字段（`CLIENT_ID` + `CLIENT_SECRET`）与混合表单（3 plain + 1 secret，含 url 模板）各一例。

---

## 10. 未做 / 登记

- `visible_in` 与 host 模式的映射表是否落成配置（D5 只确定"可映射者映射"）。
- `minWorkbuddyVersion` 是否参与兼容判定（当前完全未读）。
- 市场 `token-schema.json` 与 `02` §10 的 `userConfig` 属同一概念的两个来源；是否在 `02` 中
  正式写入"前者为后者的一种来源分支"，避免两套并存。
- `examples_zh/_en`（283 条均有）是否进入连接器详情作为推荐提问。
- `06` 的 OAuth scope 挑战（远程端点按需申请 scope）不在本方案内，另行登记。
