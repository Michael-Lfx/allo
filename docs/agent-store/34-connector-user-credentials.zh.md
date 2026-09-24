# 连接器用户凭据（key / token 类）· 技术方案

> 状态：**设计定稿；第 1–5 步已实施**（2026-09-24）。决策 D1–D6 见 §4，均有取值与代价。
> §9 的进度栏记录每步的实施状态与仍未接通的部分。
> 前置：`02-codebuddy-workbuddy-import-spec.md`（§5/§10 `userConfig → CredentialSchema`、值不入库）、
> `05-flowy-agent-store-app-server-protocol.md`（协议正文）、`06-connector-oauth-security.md`（并行的 OAuth 通道）、
> `20-tool-injection-policy.zh.md`、`21-open-decisions.zh.md`（D5=C 及 §116 落地记录）、
> `24-external-agent-skill-and-mcp-access.zh.md`（`connector/call`）、
> `16-sdk-webui-site-priority-plan.zh.md`（R22 `[credentials]` 的由来）、`web/AGENTS.md` §5（指纹与跨仓同步）。
> 用途：说明三件事——**(1)** 需要用户输入 key / token 的连接器，凭据从哪来、存在哪、何时注入；
> **(2)** 这条链路上 host / 协议 / WebUI / SDK 各自负责什么；**(3)** 本方案要修订 D5=C 的哪一条边界。
> 依据复核：本文档所有行号引用按 `main` `c96714637` 逐条核对。

---

## 1. 结论先行

| 目标 | 现状 | 本方案 |
|---|---|---|
| 让"要用户输入 key"的连接器可配置 | ❌ 无输入口；市场声明在导入期被丢弃 | 声明归一为 `CredentialSchema`，由协议下发，WebUI 用同一份 schema 渲染表单 |
| 传输模板能否到达运行时 | ❌ **导入期只留 `{type,url}` / `{type,command,args,env}`——`headers`、`staticHeaders` 未写入快照**；`env` 里的 `${NAME}` 模板只在键名恰好命中既有谓词时才转成引用 | 导入期完整保留模板（headers / staticHeaders / env / url） |
| 模板怎么被解析 | ⚠️ stdio `env` 在探针与调用两条路解析引用；headers 只在 agent 装配路径解析；url 不解析 | 引用升级为模板，**headers / env / url 三处统一解析**，单一入口 |
| 凭据存哪 | ✅ `~/.agent-store/config.toml [credentials]`（进程 env 兜底） | 沿用；**secret 与 plain 分流**（§5.3）、**按 principal 键控**（D1） |
| 凭据值是否上协议 | **刻意不上**（`21` D5=C：`config/get` 投影不含、`config/set` 白名单拒绝） | **有限放开**：只放行写入与键名元数据，值永不回传 |
| 连接器认证方式如何表达 | ⚠️ 协议的 `auth_mode` 由 transport 推导（http/sse → `oauth`），约 224 个条目因此显示"授权"入口 | 协议新增 `credential.mode`，与市场声明的认证方式对齐 |
| 市场包内可执行文件 | — | **永不执行**（§2、§8） |

**一句话**：把"需要用户输入"提升为与 OAuth **平行**的一等凭据类型，复用既有的 `secret:` 引用与
`[credentials]` 表，补齐"模板导入 → 声明下发 → 定向写入 → 统一解析"这四段，并把既有的"整值引用"
升级为模板替换。

### 1.1 三条不变量

1. **值永不越界**：不进快照、不进 DB 定义行、不进日志、协议回包只含键名与"缺哪些"。
2. **占位与注入同构**：用户填写的 key 与 OAuth token 走同一个注入器（§3.2 末节给出市场侧的既有证据）。
3. **声明只归一一次**：市场特有字段只在 host 侧解析，WebUI 与 SDK 消费同一份结构，均不含市场特有逻辑。

---

## 2. 边界（非目标）

- **不执行市场包内的任何脚本**。导入语义是 materialize + 注册为惰性组件（`script` 组件本就如此建模）。
  依据见 §3.3 第二条。若将来需要执行，须另行立项，并带显式开关、签名与门禁。
- **不做跨连接器取凭据**（市场的 `from_connector`）。凭据依赖关系会成图，需要额外的一致性语义；
  §6.1 的注入规则仍按 `{header, value_template, source}` 建模以留位。
- **不做运行环境 / 套餐门禁**（市场的 `when.active_in` 中的 `plan:*`）。host 无套餐概念，明确丢弃并登记。
- **v1 只覆盖 `type: mcp` 的连接器**（市场 283 个里 215 个）。`type: cli`（44 个）另立一轮。
- **不改动 OAuth 的存储键控**。`oauth_tokens` 同样按 `server_url` 全宿主唯一，多 principal 下会互相覆盖，
  但那是 OAuth 面的缺陷（`06`），**不在本方案的迁移范围内**，只登记（§10）。
- **不把 `[credentials]` 变成可读面**。只放开"写入 + 键名元数据"，值侧维持不可读。
- **不新增 OAuth 能力**。OAuth 三方法与其状态机不动；scope 挑战（`06` 之外的部分）不在本方案内。

---

## 3. 设计依据（既有事实，附证据位置）

### 3.1 本仓已有（不新建机制）

| 事实 | 位置 |
|---|---|
| 引用语法为 `secret:<NAME>`，全仓唯一（刻意与浏览器引擎同一字面量） | `nomifun-common/src/secret_ref.rs:27/48`（`SECRET_PREFIX` / `parse_secret_ref`） |
| 值存放为 `~/.agent-store/config.toml` 的 `[credentials]`，进程 env 兜底（config 优先） | `16-sdk-webui-site-priority-plan.zh.md` §682、`21-open-decisions.zh.md` §116 |
| 宿主启动时把该表装进**进程级**映射 | `apps/agent-store/src/main.rs:344`（`secret_ref::set_credentials`） |
| 解析口径：普通值原样透传；引用查表；**解析不到即省略该变量**（fail-closed，绝不注入字面量）；`missing` 只回报键名 | `nomifun-common/src/secret_ref.rs:100-149`（`resolve_value_with` / `resolve_env`） |
| stdio `env` 在探针与调用两条路都解析 | `nomifun-mcp/src/connection_test/mod.rs:460`、`connection_test/session.rs:49` |
| **headers 只在 agent 装配路径解析**（六处调用点） | `nomifun-ai-agent/src/factory/nomi.rs:2095`（有会话）、`:2312/2343/2401/2421/2441`（无会话，只告警） |
| 缺凭据已有上报通道：只报字段名与键名，落到会话 | `nomifun-ai-agent/src/factory/nomi.rs:2216`（`report_missing_credentials`） |
| 协议的 `auth_mode` **确实由 transport 推导**（`Stdio → none`，`Sse/Http → oauth`），与组件里存的值无关 | `nomifun-app/src/app_server_catalog.rs:199-204`；`AuthorizationRequired` 的投影在 `:206-216` |
| 身份来源是**传输绑定的 principal**，请求体不能提供或替换身份；可按 principal 撤销 | `nomifun-app-server/src/lib.rs:177-197`（`LocalPrincipal` / `principal_id`）、`:851`（`revoke_principal`） |
| 敏感键名谓词是**单一实现**，`userConfig` 与 `mcp.json env` 共用，注释明确"no second, divergent heuristic" | `nomifun-importer/src/import.rs:1319-1338`（注释 `:1319-1321` + `looks_sensitive_key` / `is_sensitive_field`） |
| 导入侧已把 `userConfig` 映射为 `CredentialSchema` 组件（`{key,type,required,sensitive}`），值一律 `[REDACTED]`、永不入库 | `nomifun-importer/src/import.rs:718-749`；`02` §10 / TC-IMP-009 |
| 注册期**优先采用组件里的结构化 `transport`**，缺失时才从 `transport_summary` 反推 | `nomifun-app/src/app_server_installer.rs:1041-1064` |
| 只读面已确立"只过 key 名"的原则 | `nomifun-api-types/src/app_server.rs:1266` |
| 值侧已有正确姿态的先例可照抄：值单向写入、加密进机器绑定 vault、永不回传、列表只给元数据 | `nomifun-api-types/src/secret.rs` |
| 凭据表**刻意**不在协议读面与写面（有测试守着） | `nomifun-app-server/src/agent_store.rs:2085-2099`（断言 `:2087-2092`、注释 "Hand-edited, never on the wire" `:2095-2097`）；`16` §688；`21` D5=C |
| 连接器 OAuth 面的三个方法（本方案平行的对象） | `nomifun-app-server/src/lib.rs:1299-1310` |
| transport 的唯一构造点是 `McpServer::from_row`，**12 个生产调用点** | `nomifun-mcp/src/types.rs:202`；`nomifun-mcp/src/service.rs` 11 处、`adapters/nomifun.rs:45` |

### 3.2 市场侧：声明是三件套（283 个连接器实测）

| 层 | 作用 | 关键字段 |
|---|---|---|
| `.codebuddy-connector/connectors.json`（508 KB / 283 条） | 声明该连接器**是否需要用户填** | `auth_mode`、`type`、`visible_in`、`minWorkbuddyVersion`、`examples_zh/_en`、`name_zh/_en`、`description_zh/_en`、`source`；顶层另有 `auth_injection_rules` |
| `connectors/<slug>/mcp.json`（239 条 server 条目） | 传输**模板** | `url`、`type`、`headers`、`staticHeaders`、`env`、`command`、`args`、`timeout`、`runtime`、`disabled`、`preAuth`（239 条**全无**） |
| `connectors/<slug>/token-schema.json`（61 个） | **表单声明**（标题、说明、取密钥入口、字段清单） | 见 §5.2 |

**一对一关联**：索引中 `auth_mode = "token"` 恰为 **61** 条，`token-schema.json` 恰为 **61** 个，
且 61/61 全部存在（缺 0）。语义为：索引声明"需要填"，同目录 schema 声明"填什么、怎么填、去哪取"。

分布：

- `auth_mode`：空 204 / `token` 61 / `server-side` 12 / `oauth` 4 / `mcp` 1 / `oneid-token` 1。
- `type`：`mcp` 215 / `cli` 44 / 空 24。
- `visible_in`：`internal` 150 / `iOA` 149 / `selfhosted` 148 / `cloudhosted` 147 / `plan:ultimate` 1 / `plan:exclusive` 1。

**传输：结构、拼写、以及它们不一致的 4 条**

- 结构：`url` 形态 **224** / `command` 形态 **15**。
- `type` 拼写：`streamableHttp` 193 · **无 `type` 16** · `stdio` 11 · `sse` 10 · `streamable-http` 7 · `http` 2（合计 239）。
- 不一致的 4 条（`cloudbase`、`edgeone-pages`、`ioa`、`laiye-adp`）：`command` 形态但无 `type`。
  即"无 `type`"的 16 条 = 12 条 url 形态 + 4 条 command 形态，两条规则（看结构 / 看拼写）单用都能落对。
- **`from_db` 的严格性不是当前风险**：市场拼写根本到不了它——导入期只按结构判断并写死 `"type"`（§3.4 第一条）。

**占位符：三处落点、两种形式，共 74 处**

| 落点 | 形式 | 例 | 计数 |
|---|---|---|---|
| headers | 整值 | `X-Api-Key: "${AGENT_EARTH_API_KEY}"` | 22 |
| headers | 内嵌 | `Authorization: "Bearer ${API_KEY}"` | 37 |
| url query | 内嵌 | `…/mcp?token=${GILDATA_TOKEN}` | 10 |
| env | 整值 | `DCS_PAT: "${DCS_PAT}"` | 5 |

落点合计：headers 59 / url 10 / env 5。形式合计：**整值 27**（headers 22 + env 5）、
**内嵌 47**（headers 37 + url 10）。按传输分：headers 59 = `streamableHttp` 55 + `sse` 4；
url 10 = `streamableHttp` 8 + `sse` 2；env 5 = `stdio` 3 + 无 `type`（command 形态）1 + `streamableHttp` 1。

承载面（决定 §3.4 第一条的断言怎么写）：224 条 url 形态条目里**只有 55 条带 `headers`**
（其中 6 条是固定值、不含任何占位符），另有 4 条带 `staticHeaders`；command 形态 15 条。

拼写**只有一种**：74/74 全为 `${NAME}`（无 `${user_config.KEY}`、`{{VAR}}`、`$VAR` 变体）。

三点必须记账：

1. `streamableHttp` 条目上还挂着一个 `env` 占位符（`yingmi-mcp`）——**http 传输不读 `env`**，
   该处是无效声明；同一连接器的 `url` 占位符才是有效落点。
2. `env` 落点里只有 4 条真的走 stdio 路径（`dcs-cloud`、`jufa-mcp-server`、`nvapp-windows-local`
   显式 `stdio`，`laiye-adp` 是 command 形态，导入后同样是 stdio）。
3. **已有引用契约的覆盖缺口**：`parse_secret_ref` 要求值为**恰好** `secret:NAME`，据此只能表达
   **27/74**（整值部分）；其余 47 处为内嵌形式，须由模板替换表达（§6.2）。

**字段与落点的对应（实测，决定 §5.4 的绑定规则）**

- 74 处占位符 = **73 个 `(连接器, 键名)` 组合 + 1 处重复**（`yingmi-mcp` 的 `YINGMI_API_KEY`
  同时出现在 `url` 与 `env`）；跨连接器重名的有 63 个不同键名，不影响对应关系。
- 61 个 schema 共 74 个字段 = **73 个被引用 + 1 个未被任何 `${}` 引用**。
- 于是按 `(连接器, 键名)` 精确匹配，**失配 0 处**——两边是同一份清单的两种表达，不是巧合。
- 那 1 个未被 `${}` 引用的字段是 `weisheng-scrm` 的 `SCRM_APP_KEY`：它在 `mcp.json` 的 `env` 里
  有**同名键，值为空字符串**——这是第三种声明形态：用空值表示"待用户填"。
  （该键名不含 `api`/`token`/`secret`/`password`/`apikey`，因此连"改写为引用"都不会发生，
  子进程会拿到空值。）

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
   带一个 55 字符的 `sk-agw-…` 默认值。全市场仅此 1 处（61 个 schema 中再无密钥形状的默认值；
   另有 4 处 plain 默认值，见 §5.3）。未验证其可用性——不对第三方凭据做验证性调用。
   归属：**源侧行为，不在本方案处置范围内**。对本仓的要求只有一条——该默认值不得进入我们的任何
   产物（§9 第 2 步的导入期丢弃 + 告警，§8 第 1 条的门禁防回归）。
2. **可执行载荷与远程下载随包分发**：`shanlong-claw` 含 `install.ps1`（25 KB）、`install.sh`（18 KB）、
   `launch.cjs`、`launch.ps1`、`launch.sh`、`connector-readonly-policy.js`、
   `verify-connector-package-scope.js`，并有 `install-url.conf` 指向
   `https://chat-cdn.tcsl.com.cn/slclaw/workbuddy-cli/`（含 `.version`、`.manifest.json`）。
   同类还有 `cli/`（3 个连接器）、`scripts/`（3 个）、`woscli` 的安装脚本、`welife-…/client.cjs`。
   处置：§2 第一条（永不执行）。

### 3.4 三处实缺（当前会导致"已配置却发不出去"）

| # | 缺陷 | 证据 | 影响 |
|---|---|---|---|
| 1 | **导入期丢弃传输的认证部分，并把 sse 压平成 http** | 两条 builder 都丢：单连接器目录 `import.rs:1198-1199` 只要存在 `url` 就写 `json!({"type":"http","url":url})`——**既不读市场的 `type`，也不写 `headers` / `staticHeaders`**；市场索引 `import.rs:1001-1028` 更彻底，连结构化 `transport` 都不写，只留 `transport_summary`（注册期再从摘要反推，`app_server_installer.rs:1041-1064`） | **55 条带 `headers` 的条目（含 59 处占位符）全部没进快照**，运行时无从解析；`staticHeaders`（4 条）同样丢弃；10 个 `sse` 连接器被当 `http` 导入（协议不同） |
| 2 | 探针 / 调用路径不解析 headers 引用（手工注册、CLI 注册、会话快照的来源才走得到这里） | `nomifun-mcp/src/connection_test/mod.rs:622-652`（`request_headers()` 只 `build_http_headers` 后判断 `Authorization`，无解析） | `headers: { "x-api-key": "secret:X" }` 的连接器，`connector/test` 与 `connector/call` 会把 **`secret:X` 字面量**当作密钥发出；同一"引用"在 stdio 与 http/sse 上行为不一致 |
| 3 | url 无解析点 | 本方案不涉及处无解析代码；市场 10 个连接器把凭据放入 url query | 含凭据的 URL 直接以字面量发出，或（模板形态）以 `${...}` 字面量发出 |
| 附 | `env` 的 `${NAME}` 模板只在键名命中既有谓词时才转成引用 | `import.rs:1348-1377`（`rewrite_secret_env`）：`looks_sensitive_key` 的五个词是 `api`/`token`/`secret`/`password`/`apikey`，**不含 `pat`、`key`** | 5 个 env 占位符里 4 个被改写成 `secret:<KEY>`（`JUFA_API_KEY`、`ADP_API_KEY`、`NVAPP_MCP_TOKEN`、`YINGMI_API_KEY`），其中 3 个在 stdio 条目上，手工填 `[credentials]` 即可工作；`dcs-cloud` 的 `DCS_PAT` 不被改写，子进程拿到字面量 `${DCS_PAT}`；`yingmi-mcp` 那条落在 http 条目上（本来就是无效落点），它的 `url` 占位符同样未被改写 |

---

## 4. 决策（D1–D6）

| # | 决策 | 取值 | 理由 | 代价 |
|---|---|---|---|---|
| D1 | 值的存储粒度 | **per-principal**：最小步命名空间化 `<principal>:NAME`，终态为 per-principal 查询面 | `[credentials]` 现为宿主级扁平映射，共享宿主上多用户互相可见；身份来源已存在（传输绑定的 `LocalPrincipal`，§3.1） | 需要迁移与 key 迁移逻辑；解析入口必须显式收 principal（§6.2）；见 §7 |
| D2 | secret 与 plain 的落点 | **secret → 凭据库**（单向写）；**plain → 连接器 transport config** | plain 值（`HOST`/`PORT`/`ENV`/`SCHEMA`）本非秘密，可见/可导出符合直觉；凭据库保持最小 | 需要给 transport config 定一层 plain 值（§5.3）；plain 值出现在协议的 `credential` 块里，不进传输摘要 |
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

沿用既有 `KIND_CREDENTIAL` 组件，扩展字段（现状仅有 `key/type/required/sensitive`）。
下表是**归一产物**，即 host 内部与 WebUI 表单消费的形态（协议面的投影见 §6.1）：

| 层级 | 字段 | 说明 |
|---|---|---|
| 连接器 | `title{zh,en}`、`description{zh,en}`、`doc_url{zh,en}`、`doc_label{zh,en}` | 市场 `token-schema.json` 的顶层键；`doc_url` 有 `docUrl_en` 变体（1 例），按语言分别取 |
| 字段 | `key`、`kind: secret\|plain`、`required`、`label{zh,en}`、`placeholder{zh,en}`、`description{zh,en}`、`default_value?` | `default_value` 是**导入期输入**：`secret` 字段的丢弃（§5.3），`plain` 字段的落进 `values`（§5.3）；协议面不下发它 |

**`kind` 的判定：`type: password` 权威，`type: text` 用一条更紧的命名谓词。**

- `type: password` → `secret`（63 个字段），这是市场自己的明确信号。
- `type: text` → 按 `credential_shaped_name` 判定（11 个字段）：名字含 `token` / `secret` /
  `password` / `apikey` / `api_key` / `_key` / `_pat` / `credential` 才算密钥。
  **不能复用 `looks_sensitive_key`**——它匹配的是裸子串 `api`，会把 tdengine 的
  `TDENGINE_API_SCHEMA` / `_HOST` / `_PORT` 判成密钥；而这三项正是带默认值
  （`http` / `localhost` / `6042`）的**普通设置**，判错会让它们从表单的普通一半消失、被塞进
  凭据库，并废掉 §5.3 为它们设计的 `values`。
  实测 74 个字段：本条谓词 **secret 65 / plain 9**；复用旧谓词（补 `key`/`pat` 后）
  得到 **68 / 6**，分歧恰好是那三项。
- `looks_sensitive_key` **仍然**就地补 `key` / `pat` 两个词，但它服务的是另一条路径：
  `mcp.json` 里**字面量**敏感值的遮蔽（§3.4 附表）——`DCS_PAT`、`SCRM_APP_KEY` 这类既不含
  `api` 也不含 `token` 的名字，此前会把包里的明文原样导入。
- 扩展 `looks_sensitive_key` 会同时影响 `userConfig` 路径，方向是"多遮蔽"，不需要单独迁移；
  但需在 `02` §10 记一笔。

i18n 回退链（host 侧实现一次，两个方向都要）：`zh → en → key`、`en → zh → key`。缺口规模：`label_en` 缺 12、`placeholder_en` 缺 16、
`description_en` 缺 14（共 74 个字段）；`title_en` 缺 1、`description_en` 缺 2（共 61 个 schema）；
`docUrl` 有 55 个，其中 `docLabel_en` 缺 **9**（另有 6 个 schema 完全没有 `docUrl`）。

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
| `secret` 字段值 | `[credentials]`，键为 `<principal_id>:<KEY>`（D1） | 只能写入；任何读面只回键名与 `missing` |
| `plain` 字段值 | 连接器 `transport_config` 的 `values`（下方形状） | 随 `credential` 块的 plain 字段回传（表单预填所必需）；不进 `connector/list` 的传输摘要 |
| `secret` 字段的 `defaultValue` | **丢弃 + 告警** | 依据 §3.3 第一条 |

**模板保留、连接时解析**（D2 的落点就此定死）：`transport_config` 保存的是**模板原文**，
不在 `credential/set` 时替换：

```jsonc
// mcp_servers.transport_config —— 以 tdengine 为例
{
  "type": "http",
  "url": "${TDENGINE_API_SCHEMA}://${TDENGINE_API_HOST}:${TDENGINE_API_PORT}/api/v1/mcp/stream",
  "headers": { "Authorization": "Bearer ${secret:TDENGINE_API_KEY}" },
  "values": { "TDENGINE_API_SCHEMA": "http", "TDENGINE_API_HOST": "localhost", "TDENGINE_API_PORT": "6042" }
}
```

**`values` 的前提（实施期发现，必须先做）**：`mcp_servers.transport_config` 不是自由 JSON，
它是 `McpTransport` 的序列化结果，且该枚举带 `deny_unknown_fields`
（`nomifun-api-types/src/mcp.rs:16`）。注册路径会先把导入产物的 transport JSON 反序列化成它
（`nomifun-app/src/app_server_installer.rs:106-114`），因此**多一个键就是注册直接失败**，
而不是被忽略。加 `values` 必须同时改：`McpTransport` 三个变体、`from_db` / `to_config_json`
（`nomifun-mcp/src/types.rs`）、会话快照侧的同名类型，以及 SDK 侧的 DTO。原文里
"`from_db` 未知键直接忽略、因此不破坏既有行"只对**读取**成立，对**注册**不成立。

选择这条的理由：`default_value` 预填在一次再编辑后仍然成立；包升级重新导入时模板与声明一起刷新；
值完全不进传输摘要。4 个 plain 默认值（`qinghu-ai.QINGHU_ENV="prod"`、tdengine 三项）由导入期
写进声明，运行时据此落进 `values`。

写入路径要求：原子写、文件权限 600（与 GitHub 官方 token 处理口径一致）；`[credentials]`
仍不进 `config/get`。**写入后必须同步更新进程内快照**，否则出现"写成功但用不上"
（进程映射在 `main.rs:344` 只装一次）。

### 5.4 字段与落点的绑定规则

绑定**按名字**，不按 `${}` 出现与否——这是 §3.2 实测（73 个 `(连接器, 键名)` 组合与 73 个被引用字段
一一对应、失配 0，另有 1 个空值特例）推出来的：

1. 字段 `KEY` 的注入点是：任意 `${KEY}` 模板（优先） → 同名 `header` → 同名 `env` 键。
2. `env` 落点上**空字符串值视为"待填"**，不是"已配置"：`SCRM_APP_KEY: ""` 必须换成引用，
   否则子进程拿到空值、静默失败。当前唯一一例就是 `weisheng-scrm`。
3. 三种落点都没有的字段：**照常渲染**（不灰显）。`staticHeaders`、`type: cli`、以及将来新增的
   落点都可能用到它，误藏会丢掉唯一的输入口。导入期登记计数——**当前 0 例**（放宽到同名键后，
   `weisheng-scrm` 命中第 1 条第 3 段）。
4. `${NAME}` 但 schema 里没有同名字段：按隐式 `secret` 处理（`required: true`，`label` 回退为 `key`）
   并告警，绝不静默丢弃。**当前 0 处**，规则是防御性的。
5. `Bearer` 之类的固定前缀一律来自模板原文，**不得按 header 名自动补**（§3.2 约束 2）。
6. **声明优先于名字启发式**（实施期发现）：一个被声明为 `plain` 的字段，即使名字长得像密钥
   （`API_HOST` 含 `api`、`API_PASSWORD` 以外的 `*_API_*`），也必须按 `plain` 处理——否则
   `API_HOST: "localhost"` 这种包里本来就带好的普通值会被遮蔽成引用，把一个本可开箱可用的
   连接器变成"要用户填"。同一个道理反向也成立：声明为 `secret` 且包里带字面量值的字段，
   一律遮蔽并告警。

---

## 6. 接口

### 6.1 协议面（WebUI 与 SDK 的共同契约）

在既有 OAuth 三方法旁新增三方法，并在连接器摘要 / 详情挂统一字段：

```jsonc
credential: {
  mode: "none" | "oauth" | "token",
  status: "not_required" | "requires_input" | "configured" | "error",
  missing: ["TDENGINE_API_KEY"],            // 仅键名
  fields: [{                                // 仅元数据，永不含 secret 值
    key, kind: "secret" | "plain", required,
    label: {zh, en}, placeholder: {zh, en}, description: {zh, en},
    value?: string,                         // 仅 plain：当前生效值（默认或用户所填）
    doc_url: {zh, en}, doc_label: {zh, en}
  }]
}
```

| 方法 | 请求 | 响应 |
|---|---|---|
| `connector/credential/get` | `{connector_id}` | 上述 `credential` 块 |
| `connector/credential/set` | `{connector_id, values: {KEY: "…"}}` | 新的 `credential` 块（不含值） |
| `connector/credential/clear` | `{connector_id, keys?: [KEY]}` | 新的 `credential` 块 |

**`auth_mode` → `credential.mode` 的完整映射**（283 条全量，不留未定义项）：

| 市场 `auth_mode` | 条数 | `credential.mode` | 说明 |
|---|---|---|---|
| 空 | 204 | `none` | 无需凭据 |
| `token` | 61 | `token` | 本方案的主对象，与 61 个 `token-schema.json` 一一对应 |
| `oauth` | 4 | `oauth` | 走既有 OAuth 通道 |
| `server-side` | 12 | `none` | 凭据由服务端持有，客户端不输入 |
| `mcp` | 1 | `none` | 语义未定，v1 归 `none` 并登记（§10） |
| `oneid-token` | 1 | `none` | 需 iOA 内部签发的 token，host 无签发能力，登记（§10） |

**这是有意为之的行为变更，实现者不要当副作用**：协议的 `auth_mode` 今天由 transport 推导
（`app_server_catalog.rs:199-204`：`Stdio → none`，`Sse/Http → oauth`），而导入期对 url 形态
一律写 `"type": "http"`，所以**今天约有 224 个连接器一律显示"授权"入口并处于
`authorization_required`**（`:206-216`）。改用 `credential.mode` 后：61 个显示"填入凭据"、
4 个保留"授权"、其余显示无需认证。这正是要修的 bug（`token` 类连接器拿到 OAuth 入口）。

其余约定：

- **状态词表收敛**：UI 只实现 `not_required / requires_input / configured / error` 一种四态；
  OAuth 的 `authenticated` / `not_authenticated` 映射进同一套。
- **`error` 的产生者只有一处**：`connector/test`。探测失败且判定为认证类（401/403）时写
  `error` 并落在既有的 `mcp_servers.last_test_status = Error`；`credential/set` 成功即清除它。
  `missing` 非空时 `requires_input` 优先于其余状态。
- **`connector/test`** 需能单独回报"缺凭据"（typed error），而非笼统失败。
- **`auth_mode` 去重**：协议内以 `credential.mode` 表达认证方式；UI 不得继续用 `auth_mode`
  决定按钮形态，否则 61 个 `token` 类连接器会得到 OAuth 授权入口。
- 注入规则建模为 `{header, value_template, source}`；v1 的 `source` 仅取本连接器凭据（D4）。

### 6.2 运行时解析

- `secret_ref` 由"整值引用"扩展为模板替换（§5.1），实现为单一函数。
- **单一入口，且显式收 principal**：

  ```
  resolve_transport(transport: &McpTransport, principal: Option<&str>) -> ResolvedTransport
  ```

  签名必须带 principal，否则 D1 无从落地。三条路径的身份来源不同：

  | 路径 | principal 来源 | 取不到时 |
  |---|---|---|
  | `connector/test`、`connector/call` | 协议连接绑定的 `LocalPrincipal.principal_id`（§3.1） | 不会发生（连接必然有 principal） |
  | agent 装配（`nomi.rs:2095` 等） | 只有 `conversation_id`，需补一条 conversation → owner principal 的查询 | **secret 一律视为缺失**（fail-closed），沿用既有 `report_missing_credentials` 上报 |

  **绝不回退到宿主级取值**：共享宿主上那等于串号。宿主级的旧键只对**宿主操作者本人**
  （本地 owner principal）回退可见（§7）。
- 解析后的 transport 由这一个方法产出，probe / call / agent 三条路共用，消除 §3.4 第二条的分叉。
  `McpServer::from_row` 的 12 个生产调用点改为"取模板 → 过入口"，其中 `connector/list` 只投影
  **模板 + 字段元数据**，不投影 `values`（§5.3）。
- 三处落点全部覆盖：headers、env、url。
- 失败模式：缺凭据 → 不发起请求 → 返回 `missing credential: <KEY>`；日志只记键名。
  **`report_missing_credentials`（`nomi.rs:2216`）是既有机制，此处是复用它并把它提升为协议 typed error，
  不是新建一套缺凭据上报。**

### 6.3 WebUI

- 由 §5.2 的 schema 驱动单一表单组件：标题 / 说明 / 取密钥入口（带语言回退）/ N 个字段。
  `secret` 字段为掩码输入、**不预填、不回显**；`plain` 字段预填 `value`。
- 入口与徽标：`mode = token && status = requires_input` → 「填入凭据」；`mode = oauth` 保留既有「授权」。
  徽标统一为四态，并显示缺失项数量。
- 第三方文案（含市场作者写的安全说明，如"仅存本机，请勿发给对话里的智能体"）随 schema 下发；
  SPA 自身 chrome 仍走既有 i18n。
- **错误展示复用刚落地的那条通道**：`auth_status.error` 已是"浏览器步之后失败"的唯一出口，
  抽屉里也已有渲染（`web/src/components/CatalogView.tsx:247` 起）。token 模式的 `error`
  必须走同一个出口，一个连接器只能有一处错误展示，不新加第二种错误面。
- **写入后的状态刷新复用既有轮询节奏**：凭据写入是同步的，没有需要等待的浏览器步，
  因此不需要 `waitForAuth` 那套等待；但"刷新到新状态"应按已落地的探针节奏轮询
  （`web/packages/client/src/connectors.ts`），不要为凭据另写一个轮询。

### 6.4 SDK 与跨仓

- `ConnectorClient` 新增 `credentials(id)`、`setCredentials(id, values)`、`clearCredentials(id)`；
  协议 DTO 镜像 host，含 `LocalizedString`。既有的 `waitForAuth`（`:133`）是 OAuth 专用等待，
  凭据写入不需要对应物；但新增方法的返回值应与 `OAuthStatusView` 同形（状态 + 原因）。
- 新增方法会改动 `web/packages/client/src/http-transport.test.ts` 的 `DOCUMENTED_ROUTE_SPLIT`、
  站点 `content/docs/{zh-CN,en-US}/typescript-sdk.md` 的计数与常量、`changelog`，并
  **bump `fp-8 → fp-9`** 后两仓同步（门禁：`check:fingerprint`、`check:release-sync`）。

---

## 7. 兼容与迁移

| 项 | 处置 |
|---|---|
| 传输拼写与结构 | 导入期按 §3.2 的拼写表归一为 `http` / `sse` / `stdio`（`sse` 不再被压平成 `http`）；`from_db` 保持严格 |
| 市场 `auth_mode` | 作为声明值导入，映射为 `credential.mode`（§6.1 全量表）；与 transport 推导值区分 |
| `[credentials]` 键控（D1） | 新写入一律带 `<principal_id>:` 前缀；**无前缀旧键保留为"宿主级"并登记**，只对本地 owner principal 回退可见，不对其他 principal 可见 |
| agent 装配路径缺身份 | conversation → owner principal 查询缺失时按 fail-closed 处理（§6.2），不静默降级到宿主级 |
| OAuth 连接器 | 行为不变；仅状态词表映射。`oauth_tokens` 的存储键控**不动**（§2） |
| 新增 DTO 字段 | 均为可选字段，旧客户端忽略即可 |
| 指纹 | 本方案实施时为 `fp-9`；本文档本身不含 wire 变更 |

---

## 8. 风险与失败模式

1. **默认值泄密回归**（§3.3 第一条）。缓解：导入期丢弃 `secret` 字段的 `defaultValue` 并告警；
   对本仓加一条机械门禁（`secret` 类字段带 `defaultValue`，或 `defaultValue` 形状像密钥即失败），
   使该行为无法经导入进入我们的产物。
2. **导入期静默丢字段的回归**（§3.4 第一条）。这是当前最贵的一条：`headers` 被丢掉时没有任何报错，
   表现为"连接器装好了但永远认证失败"。缓解：导入期把 `headers` / `staticHeaders` / `env` / `url`
   全量写入模板，并加一条测试断言"源里带 `headers` 的 55 条与带 `staticHeaders` 的 4 条，
   导入后模板里仍在"（连同一个反向断言：10 个 `sse` 条目导入后 `transport_type` 仍是 `sse`）。
3. **`secret:` 字面量外泄**（§3.4 第二条）。缓解：单一解析入口 + 集成断言（外发 header 必须是解析后的真值，
   日志不得出现值）。过渡期内，未解析的引用宁可导致"缺凭据"错误，也不得原样发出。
4. **URL query 携带凭据**（市场 10 例）。凭据会进入代理日志、`Referer`、服务端访问日志；
   导入期单独告警，不作为推荐形态。
5. **`env` 上的无效落点**（`yingmi-mcp` 的 http + env 组合，§3.2）。导入期对"http/sse 条目上出现 `env`"
   告警——那是个永远不生效的声明。
6. **共享宿主上的凭据互见**（D1 未完成前）。多用户场景下必须先完成 per-principal 键控；
   实现期不得以"宿主级兜底"作为过渡手段，那会在过渡窗口里就发生串号。
7. **包内可执行载荷**（§3.3 第二条）。导入永不执行；文档与 UI 明确这一点。
8. **明文落库**：`oauth_tokens.access_token` 现为明文（实测 40 字符），全仓无 DB 层加解密 helper；
   本方案不新增该能力，但凭据表的最小化（D2）与不可读（§2）降低了暴露面。
9. **多字段表单的可用性**：4 字段（`tdengine`）含 plain 与 secret 混合，UI 必须区分二者
   （哪一项会被保存到凭据库、哪一项会写进连接器配置）。

---

## 9. 实施顺序与验收

| 步 | 内容 | 验收 |
|---|---|---|
| 1 | 修 §3.4 第一、二、三条：导入期完整写入模板（headers / staticHeaders / env / url）+ 按拼写表归一传输；`request_headers()` 接入解析入口；url 解析 | 导入测试：url 形态条目带 `headers`，10 个 `sse` 保持 `sse`；`nomifun-mcp` 单测：含引用的 headers / url 发出解析后的真值。零协议变更，可独立合入 |
| 2 | 导入层：读 `token-schema.json` + 市场 `auth_mode` → 归一为 `CredentialSchema`（§5.2）；`${NAME}` 按 §5.4 绑定规则把**声明的 secret 字段**转成 `${secret:NAME}`；`secret` 字段的 `defaultValue` 丢弃并告警；`looks_sensitive_key` 补 `key`/`pat`，字段判定另用 §5.2 的更紧谓词 | 导入测试：字段与 i18n 回退齐全；`kind` 实测 65 secret / 9 plain；`weisheng-scrm` 的空值形态被识别为待填；未声明的占位符告警而非静默转发；以 §3.3 第一条为夹具，断言 `[REDACTED]`、值不进快照与 DB |
| 3 | 运行时：模板解析（含 plain 的 `values`，需先按 §5.3 扩展 `McpTransport`）+ 显式 principal（§6.2）+ typed `missing credential`；**per-principal 键控随第一次写入一起落地**（D1 的最小步，不留到第 6 步） | 集成测试：内嵌模板被正确替换；plain 字段取到默认值/用户值；缺凭据时不发起请求且错误只含键名；日志无值；两个 principal 各自 `set` 同名 KEY，探针/调用各取各的 |
| 4 | 协议 + SDK：三方法 + `credential` 块 + `mode` 映射表 + 四态词表 + `fp-9` + 站点同步 | `cargo test -p nomifun-app-server`；`check:fingerprint` 十处落点一致；`check:release-sync` 计数一致；站点 `check:docs-sync` 0 drift |
| 5 | WebUI：schema 驱动表单 + 四态徽标 | 组件测试（i18n 回退、`secret` 不预填）；`cd web && bun run typecheck && bun run test`；手测双字段表单与混合表单 |
| 6 | 存储终态（D1）：per-principal 查询面 + 旧键迁移 | 无前缀旧键只对 owner principal 可见；迁移测试；两个 principal 互不可见 |
| 7 | D4 / D5 / D6 的登记项 | 各自立项 |

第 1、2 步都不含协议变更，可以先落。第 3 步的键控之所以提前到写入路径诞生时，是因为第 4 步
一旦开放写入，第 6 步之前落盘的每一个键都要再迁移一次——把最小步（命名空间化）提前，第 6 步
就只剩终态查询面与旧数据搬迁。

### 9.2 实施进度（2026-09-24）

| 步 | 状态 | 落地内容与遗留 |
|---|---|---|
| 1 | ✅ 完成 | 导入期保留 `headers`/`staticHeaders` 并按拼写表归一（`sse` 不再被压平）；`secret_ref` 增加 `${secret:NAME}` 模板形式；探针 / 调用 / DB 行 / 会话快照 / ACP 五条路径共用解析入口并 fail-closed，缺凭据返回新增的 `MCP_MISSING_CREDENTIAL`（422） |
| 2 | ✅ 完成 | `token-schema.json` 归一为 `credential` 组件（字段 + i18n 双语言 + 取密钥入口）；`${NAME}` 按 §5.4 升级引用；secret 默认值丢弃并告警；`looks_sensitive_key` 补 `key`/`pat`，字段判定另用更紧谓词；`auth_mode` 取自市场索引（写进 `connector` 组件） |
| 3 | ✅ 完成 | **已完成**：`values` 层（四处类型：`McpTransport` / `McpServerTransport` / `SessionMcpTransport` / 网关 `McpTransportParam`）；`TransportScope` + `resolve_request_string` 两类命名空间分流；`<principal>:NAME` 键控与安装所有者可见性规则；探针与工具调用两条路径都接通调用者身份，stdio 会话池按"解析后的 env"复用（凭据不同即不复用）。**装配路径无需再改**：`load_user_mcp_servers`、host 声明合并与 ACP 构建都在 `is_instance_owner = authority.controls_host()` 之后，只有安装所有者本人的会话会注入 MCP，因此按宿主解析就是准确语义（实施时原以为这是遗留，核对门禁后确认不是） |
| 4 | ✅ 完成 | 协议类型（`credential` 块 + 字段 + 双语言）；目录投影按**调用者**给出 `mode`/`status`/`missing`/`fields`；`[credentials]` 写入面（`toml_edit` 最小改动，注释与排版保留、原子落盘、写完重载进程内映射）；`connector/credential/get\|set\|clear` 三方法 + HTTP/WS 路由 + `ConnectorCredentialProvider` seam + 组合根接线；SDK 三个方法与协议类型；**`fp-8` → `fp-9`**、方法计数 `48 / 73` → `51 / 76`、两仓同步 |
| 5 | ✅ 完成 | WebUI：`ConnectorCredentialForm`（schema 驱动，标题/说明/字段/取密钥入口全部由 host 下发且带回退；`secret` 掩码不预填不回显，`plain` 预填；空值不提交）；抽屉按 `credential.mode` 给入口（`token` → 「填入凭据」，`oauth` → 既有授权，`none` → 无），徽标统一为四态并带缺失项数量；token 模式的 `error` 复用既有 `drawer-hint is-error`（一个连接器一处错误展示）；写入后按探针同形刷新（`get` + `status` + `list`），不新写轮询 |
| 6 | ⬜ 未开始 | 存储终态（D1）：per-principal 查询面 + 旧键迁移 |

**第 5 步期间修掉的一处第 4 步接线缺口**：`credential.mode` 当初挂在凭据声明上，
但导入期从不往 `credential` 组件写 `auth_mode`（归一值写在 `connector` 组件里），
`declaration.auth_mode` 因而恒为 `none`——61 个 `token` 连接器一律投影成 `mode: none`
（正是本方案要修的那个 bug 的镜像），而 14 个 `server-side` / `mcp` / `oneid-token`
连接器没有 token-schema、也就没有声明，落到 transport 兜底又变回 `oauth`。现引入
`ConnectorCredentialSource`：模式取自 `connector` 组件（每个市场连接器都有），表单取自
`credential` 组件（只有 61 个有），transport 兜底只留给非市场来源的手工注册服务器。
单元测试手写 payload 因而看不见这段接线，补了一条 e2e 走完整条链路：市场目录 → 安装 →
`connectors` 投影 → `credential/get` → `set` → 磁盘上的 `[credentials]` → `clear`。

第 3 步的 raw 计数：`nomifun-common` +6 单测、`nomifun-importer` +2、`nomifun-mcp` +5、
`nomifun-app-server` +0（既有 173 条回归通过）。第 4 步新增：`nomifun-app-server` 4（agent_store
写入面）、`nomifun-app` 8（投影与写入面）、`web` 三个 SDK 方法（路由计数锁步校验）。
第 5 步新增：`web` 22（表单与四态词表，含渲染测试）、`nomifun-app` 1 条 e2e（上面那条链路）。

### 9.1 端到端验收（活体）

以 mock key-based MCP server 为对象（沿用既有 live 脚本形态）：

1. 未填凭据：`connector/test` 返回 `requires_input`，且 mock **未收到任何请求**；
2. `credential/set` 后：`connector/test` 通过并返回工具表；
3. 外发 header 为模板解析后的真值（含模板中的 `Bearer ` 前缀）；host 日志只含键名；
4. `credential/clear` 后回到 `requires_input`；
5. 覆盖四种形态各一例：双字段（`CLIENT_ID` + `CLIENT_SECRET`）、混合表单（3 plain + 1 secret，含 url 模板）、
   `sse` 传输（确认未被压平）、空值待填（`SCRM_APP_KEY: ""`）。

其中"填了凭据之后探针真的带上解析后的 header"这一段仍未有自动化覆盖：第 5 步的 e2e
止于 `[credentials]` 与投影一致，未起一个真的 MCP server 去收 header。补法是把这条
变成 §9.1 的 live 脚本（需要 mock server + 探针），或复用 `nomifun-mcp` 的连接测试夹具。

---

## 10. 未做 / 登记

- `visible_in` 与 host 模式的映射表是否落成配置（D5 只确定"可映射者映射"）。
- `auth_mode = mcp`（1 条）与 `oneid-token`（1 条）的语义；v1 归 `none`。
- `oauth_tokens` 按 `server_url` 全宿主唯一、多 principal 互相覆盖的缺陷（§2 排除，属 `06`）。
- `minWorkbuddyVersion` 是否参与兼容判定（当前完全未读）。
- `preAuth`（239 条全无，导入期默认成 `oauth`）：是否需要在 `02` 中承认该字段。
- `mcp.json` 的 `runtime` / `disabled` / `timeout`：导入期是否读取（当前全部忽略）。
- 非占位 `env` 键（`NO_PROXY`、`PYTHONUTF8`、`SCRM_BASE_URL` 等）是否需要进入连接器的 plain 值。
- `looks_sensitive_key` 扩展后对 `userConfig` 路径的影响，是否需要在 `02` §10 记一笔。
- 市场 `token-schema.json` 与 `02` §10 的 `userConfig` 属同一概念的两个来源；是否在 `02` 中
  正式写入"前者为后者的一种来源分支"，避免两套并存。
- `examples_zh/_en`（283 条均有）是否进入连接器详情作为推荐提问。
- `06` 的 OAuth scope 挑战（远程端点按需申请 scope）不在本方案内，另行登记。
- **`ConnectorStatus` 仍是 transport 推导的**：UI 已改为优先用 `credential` 四态（第 5 步），
  但 `connector/list` / `connector/status` 的 `status` 字段对未探测过的 token 连接器仍是
  `authorization_required`——直接读该字段的 SDK 使用者会得到「需要授权」。要根除得让
  `summary_status` / `probe_status` 改看 `credential.mode`（含"缺凭据 → `installed`"），
  那是协议语义变更，另行立项。
- **既有快照的 `auth_mode` 是市场原值**：归一（空 / `server-side` / `mcp` / `oneid-token` → `none`）
  只写进新导入的 `connector` 组件；本方案之前导入的 204 个连接器其 `auth_mode` 是
  `preAuth` 为空时的字面量 `"oauth"`，因此仍会显示授权入口，需要重新导入该市场条目才会归一。
  是否要做一次性快照搬迁，另行登记。
