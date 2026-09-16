# Agent Store 文档索引

> 最后核对：2026-09-15（**安装面五动词「做真事」：`install/run` 可重入（同快照不产生第二个 Preset）、`uninstall` 真正释放产物（skill 目录 / Preset / `mcp_servers` 行，已不在算成功、失败保留 `installed=1`）、`disable`/`enable` 真正移动运行时状态（skill 例外＝目录标记 `skill_disable_flag_only`）、新增结构化 `outcomes`（`action` 八值 + 稳定 `code` 闭集）、`store/install-entry` 版本感知（升级只有「卸载再安装」一条路）、client 新增 `store` 子客户端（20 例）、宿主管理面类型搬进 `protocol`、商店假「更新」控件已摘除；**协议指纹 bump 到 `2026-09-15`**（现有 DTO 加字段，8 处代码/夹具 + 2 处站点文档，方法计数不变）**；同日：**`agent/run` 加稳定码 `preset_disabled`、`team/run` 加 `agent_disabled`（点名成员，检查在 `resolve_team_members` 每次运行、先于 Connector 栅栏）**；2026-09-14：**`20` §9.5 + `16` §5.3：`config/get.mcp` 新增 `adopted`（宿主是否真的把这批声明注入会话——`servers` 描述文件、`adopted` 描述宿主），设置页据此分出「使用中 / 未使用 / 无法判断」三态；**协议指纹 bump 到 `2026-09-14`**（8 处代码/夹具 + 2 处站点文档，方法计数不变）**；同日：**`16` R16 追记 · 消息渲染：设置页三个错误字段改判别式联合 `ConfigMessage`，宿主散文不再过 `t()`（i18next 的 `looksLikeObjectPath` 会把 `mcp.json is not valid JSON: …` 截成冒号后半段）**；2026-09-13：**`16` R16 追记 + `20` §9.4：设置页新增只读 `mcp` 分区渲染 `config/get.mcp`（nav 三→四；该批零协议变更）；同日上一轮：`20` §7.9.1–§7.9.3/§9.4 补齐参考实现文档里的全部可选字段（`cwd` / `bearerTokenEnvVar` / `startupTimeoutMs` / `enabledTools` / `disabledTools`）并把 `headers` 的 `secret:NAME` 语义在三条装配路径收敛为一个函数；server 级工具过滤在注册**之前**裁剪；**无协议变更**，指纹保持 `2026-09-13`**；再上一轮 2026-09-12：**`21` D14 + `20` §7.9/§9.3：Agent Store 支持 Kimi 式 `~/.agent-store/mcp.json` 声明 MCP（用户级、纯内存注入、不投影进 `mcp_servers`），协议指纹 bump 到 `2026-09-13`**；更早 2026-09-11：新增 `22-webui-productionization.zh.md`；`16` C 档二次复核改判；**版本框架订正：发版前只有一个版本，统一称 v1，见 `16` §7 决策 4**；引用/口径统一 + 证据与 `TC-*` 正文并入编号文档；**`20` 重构：工具面表达层定为 `~/.agent-store/config.toml [tools]`，新增 `16` §7 决策 5**；**`20` Step 7 全部落地：`team/run` + Team 层委派放行，协议指纹 bump 到 `2026-09-12`**）
> 用途：本目录文档的地图、权威顺序与状态图例。新会话/新成员先读本文，再按需深入。
> 维护约定：新增文档编号顺延（当前到 `24`）；状态变更时同步更新本索引与文档头部“更新”行；公共契约变更先写 `16-sdk-webui-site-priority-plan.zh.md` §7（决策记录），再改基线文档。

## 状态图例

| 标记 | 含义 |
|---|---|
| 🧊 基线（发版前可改） | 架构/公共契约基线的**当前正文**；**未正式发版，故不是冻结**——改动走决策记录（`16` §7 决策 4：发版前只有一个版本，统一称 v1） |
| ✅ 已实现 | 代码落地并有实证（指向证据页） |
| 🔧 部分实现 | 主体落地，尾项未完成 |
| 📋 计划 | 已排期/已定义，未开工或进行中 |
| 📎 证据 | 运行时实测快照（有时间戳，非契约） |
| 🗄️ 历史 | 被后续文档取代，保留追溯 |

## 一、权威基线（先读这组）

| 文档 | 内容 | 状态 |
|---|---|---|
| `00-architecture-decision.md` | 架构边界、组件职责与取舍 | 🧊 基线（部分已实证） |
| `01-domain-model.md` | 领域模型（Agent/Team/Skill/Connector、Execution） | 🧊 基线 |
| `02-codebuddy-workbuddy-import-spec.md` | Importer 规格（CodeBuddy/WorkBuddy 导入） | ✅ 已实现 |
| `03-codebuddy-compatibility-matrix.md` | 三态兼容矩阵与推导规则 | ✅ 规则已实现 |
| `04-flowy-agent-store-runtime-adapter.md` | Runtime Adapter（Preset → ExecutionParticipant） | 🧊 基线（单 Agent 已实证） |
| `05-flowy-agent-store-app-server-protocol.md` | **App Server 协议 v1**（发版前只有一个版本） | ✅ 现行正文（可改）+ 单 Agent 实测 |
| `06-connector-oauth-security.md` | Connector OAuth 与安全模型 | 🧊 基线（OAuth 已实证） |
| `07-typescript-sdk.md` | **TS SDK v1**（包名以 `12` 为准；发版前只有一个版本） | ✅ 现行正文（可改），主体已实现 |
| `08-flowy-web-integration.md` | Flowy/Web 集成面 | 🧊 基线 |
| `10-public-contracts.md` | 公共契约（状态、事件、错误、幂等、认证） | 🧊 基线 |
| `11-webui-production-readiness.md` | WebUI 生产就绪差距清单 | 📋 规划中 |
| `17-plugin-spec.zh.md` | **插件规范（兼容层）**：接受的格式、归一化、安全边界 | ✅ 现行正文（未发版，可改；`17` §10 登记未实现项） |
| `18-marketplace-spec.zh.md` | **市场规范（兼容层）**：目录形态、清单发现、获取晋升、发布与客户端契约 | ✅ 现行正文（未发版，可改；`18` §11 登记偏差） |

## 二、计划与执行（按权威顺序）

| 文档 | 内容 | 状态 |
|---|---|---|
| `16-sdk-webui-site-priority-plan.zh.md` | **当前执行顺序（四方向）**：① SDK + 站点（配对） ② 插件与市场规范 ③ WebUI ④ 待立项 | 🔧 批 0–7 已收口（三闭环验收通过、`17`/`18` 现行正文）；剩余见 §5.2 剩余任务总表 **R1–R34**（✅ 23 · 🟡 4 · ⏸ 7）；C 档已于 2026-09-11 二次复核改判 |
| `21-open-decisions.zh.md` | **开放决策书**：D1–D16，逐条带 ⭐ 默认建议与解锁范围（`16` §5.2 剩余任务总表的拍板入口） | ✅ 已拍板（2026-09-10；D13 卡点四档已批准，含 2026-09-11 C 档二次复核改判；D14 MCP 声明文件接入路径 2026-09-12 拍板 ①A ②C ③C；D15 skill 的 `disable` ＝目录标记、D16 商店假「更新」控件摘除，均 2026-09-15） |
| `22-webui-productionization.zh.md` | **WebUI 生产化立项**（方向四 + WP-5 协议词汇与概念对齐，来源 `16` R33）：11 项拆为**安全类 / 可观测类 / 功能类 / 协议词汇对齐**，逐条给「可验收条目 + 边界 + 依赖」；含**不做假保护**红线与 V1–V4 未决 | 📋 已立项（2026-09-11），未排期 |
| `24-external-agent-skill-and-mcp-access.zh.md` | **外部 Agent 使用已安装 Skill / MCP**：Skill 文件读面（`skill/files`·`skill/file`）+ MCP 调用代理（`connector/call`，allowlist 默认全关、凭据不出宿主）；含非目标、验收口径、指纹与跨仓步骤 | 🔧 阶段 1 已落地；阶段 2 进行中（MCP 工具调用能力已落地并验证，策略/路由/TS/指纹未接通） |
| `19-webui-codex-alignment.zh.md` | **WebUI 子计划**：Codex app 体验对齐基线 + 四层工作包 W1–W14 | 🔧 部分落地（W1 / W5 / W8 / W13 已交付；其余见 R 表） |
| `15-store-chain-and-protocol-vnext-plan.zh.md` | 四链路闭环 + 协议词汇与概念对齐（WP-1~WP-7；原称「协议 vNext」） | 🔧 WP-1/2/3/4/6 完成，WP-7 模型选择器完成；WP-5 顺延至 `16` |
| `13-p0-execution-plan.md` | P0-A/B/C/D 执行细则与出口条件 | 🔧 P0-A/B 已关闭；P0-C/D 的 OAuth 运行时证据已完成（`06-connector-oauth-security.md` §12，26/26 PASS） |
| `16-sdk-webui-site-priority-plan.zh.md` §6 / §7 | 阶段基线与决策记录（原 `agent-store-v1-roadmap.md` §1–§6 / §9 / §10） | 🧊 基线 |
| `agent-store-v1-test-cases.md` | **TC 总索引 + 公共口径**（§1/§2；`TC-*` 逐条正文归各归属文档，见其 §3） | 🧊 基线 |
| `12-sdk-packaging.md` | SDK 封装与发行（P0/P1 已落地，P2 待做） | 🔧 进行中 |
| `14-binary-size-trimming.zh.md` | 二进制瘦身（feature gate 已落地） | 🔧 待 release 重构建验证 |
| `09-release-readiness.md` | 发布门禁与准入判断（§9 是**带日期的评估快照**，非门禁结果） | 📋 blocked（快照见 §9，`assessed_at: 2026-09-04`；2026-09-15 仅订正事实与登记本批，未重跑门禁） |
| `开发计划.md` | 早期开发总纲 | 🗄️ 历史（见其头部指针） |
| `技术方案.md` | 早期技术总纲 | 🗄️ 历史（见其头部指针） |
| `agent-store-v1-roadmap.md` | 早期路线与测试验收计划 | 🗄️ 历史（阶段与决策已并入 `16` §6／§7） |

## 三、证据（运行时实测快照）

| 文档 | 覆盖 |
|---|---|
| `02-codebuddy-workbuddy-import-spec.md` §14 | TC-IMP-001~009（Importer） |
| `06-connector-oauth-security.md` §13 | OAuth 登录 → 注入 → 401 刷新（真实 MCP 服务，引擎层） |
| `06-connector-oauth-security.md` §12 | WP-3 P0-C/D OAuth 协议面证据（TC-OAUTH-001/002/004，SDK 公共面 26/26 PASS） |
| `13-p0-execution-plan.md` §14 | TC-RT-001 单 Agent Run（mimo-v2.5 真实模型） |
| `15-store-chain-and-protocol-vnext-plan.zh.md` §10 | WP-2 四链路 live（C1–C4 + S1 + TC-CONN-002，20/20 PASS） |
| `13-p0-execution-plan.md` §15 | WP-3 P0-A/B（TC-RT-004/002/010 + TC-API-002/003 + TC-RT-005/006；18+10 PASS） |

> 上表指向**已并入编号文档的现行证据正文**；独立证据页与设计记录原件已于 2026-09-11 删除，正文见上表的并入位置。

## 四、设计记录

| 文档 | 内容 | 状态 |
|---|---|---|
| `06-connector-oauth-security.md` §14 | MCP OAuth 动态客户端注册设计 | ✅ 已实现 |
| `20-tool-injection-policy.zh.md` | **Store 会话工具注入策略**：三份 config.toml 哪份对工具面生效、逐项取舍（含「可配置 vs 只能写在代码里」表达层）、两类延迟、表达机制与坑、`nomi_delegate` 特例、实施步骤 Step 1–9 + §9.1 落地记录 + §9.2 Step 7 三批 + §9.3 MCP 声明文件 + §9.4 可选字段全量支持 | ✅ Step 1–9 已落地（2026-09-11：`team/run` + Team 层委派放行；2026-09-12：`~/.agent-store/mcp.json` 声明 MCP，见 §7.9/§9.3；2026-09-13：补齐全部可选字段 + `headers` 收敛，见 §7.9.1–§7.9.3/§9.4；两处端到端断言缺口分别登记在 §9.2.2 与 §9.3/§9.4） |

## 阅读顺序建议

- **接手开发**：本索引 → `16`（当前执行顺序与卡点处置）→ `15`（当前顺序）→ `05`/`07`（协议与 SDK 基线）→ `13`（P0 细则）→ 证据页
- **了解架构**：`00` → `01` → `04` → `05` → `10`
- **对接 SDK**：`12` → `07` → `05`
- **生产化评估 / 安全加固**：`22`（WebUI 生产化立项）→ `11`（差距清单）→ `06`（Connector OAuth 与安全模型）
- **发布评估**：`09` → `agent-store-v1-test-cases.md`

## 已知待修（P1）—— 全部收口（2026-09-11）

1. **冻结文档状态行**：✅ 已订正（2026-09-11）——`00/01/04/06/08` 头部与 `15`／`16` §6 的「架构/计划/路线冻结」改为「基线／现行」（发版前非冻结，见 `16` §7 决策 4）；文档日期快照仍以本索引为准。
2. **App Server stdio 口径**：✅ 已统一（2026-09-11）——`00/05/06/07/08/10` 的 App Server 传输表述均标注「V1 不含 stdio」（`16` §7 决策 2）；MCP STDIO 连接器（`06` §7）不受影响。
3. **证据文件位置**：✅ 已定（2026-09-11）——证据正文已整体并入编号文档（`02` §14、`06` §12/§13/§14、`13` §14/§15、`15` §10），本索引只登记并入后的位置；**7 份独立原件已删除**，引用同步改指并入位置（含 3 处 Rust 源码注释与 `roadmap`）。
4. **规划文档重叠**：✅ 已定（2026-09-11）——`开发计划.md`/`技术方案.md`/`roadmap` 均保留为历史追溯、正文不再更新，现行内容以 `16` §6／§7、`13`、`15` 为准（各文件头部已有指针/合并标注）。
5. **TC 正文归属**：✅ 已收敛（2026-09-11）——`TC-*` 逐条正文归各归属文档（`02` §13、`13` §10、`05` §14、`06` §11、`19` §9），`agent-store-v1-test-cases.md` 精简为「§1/§2 公共口径 + §3 归属总表」；`13` §11 的环境与等级口径收敛为指向 §1/§2 的指针（消除重复副本）。
6. **「冻结」措辞收尾**：✅ 已收敛（2026-09-11）——除语义性「快照冻结」（TC-RT-002 / ResolvedPresetSnapshot 等）外，`00` §10、`10`、`12`、`13` 及 `16` 任务表的「公共契约冻结／冻结文档／v1 冻结」统一改为「基线／现行正文」；`开发计划.md`／`技术方案.md`／`roadmap` 为历史文档，原文保留（头部已标注）。

## 本轮（2026-09-16 ~ 2026-09-18）

三批落地 + 两轮收尾（站点迁出、仓库级整理），逐条记录与全部读数在 `16` §8；本索引只登记**受影响的文档**与**跨仓事项**：

1. **目录页改为「专家 / 技能 / 连接器」三名词 tab**（`16` §8.1）：取代 `16` §5.2 的 D-W13-2 四页签形态；市场注册表搬进设置对话框（`MarketSettingsSection`），导入面搬进由技能 / 连接器 tab 打开的对话框。**本批无协议变更**，指纹不动。
2. **`store/list` 新增 `published_at`**（`18` §4.2 字段语义 + §9.3 落地记录）：`05` §5 的 `store/list` 投影表已加该字段；指纹 `2026-09-15` → `2026-09-16`（现有 DTO 加字段，方法计数不变）。
3. **MCP 声明文件从只读变可读可写**（`21` D17 + `05` §4.10 + `20` §7.9）：新增 `config/get-mcp` / `config/set-mcp` / `config/set-mcp-enabled`；`10` §7 补五条 `mcp_*` 错误码。指纹 `2026-09-16` → `2026-09-17` → `2026-09-18`；方法计数守卫 `46 / 65` → **`46 / 68`**。
4. **站点迁出后的收尾（提交 `772890fed`）**：站点已迁为独立仓（`C:\workspace\agent-store-site`），
   本仓 `site/` 只剩指向说明。与之耦合的**两道守卫已在本仓退场**，而不是留着说假话：
   `scripts/check-docs-sync.mjs`（+ 其单测）在站点目录消失后读数变成 `0 page(s)`——**恒绿且不再
   守卫任何东西**；`web/packages/client/src/docs-drift.test.ts` 直接 `ENOENT`。`check` 链与
   `scripts/scripts.json` 的登记同步移除。本仓留下的只有 `http-transport.test.ts` 的**本地**计数
   守卫（46 映射 / 22 无 HTTP 绑定）。**两条跨仓待办——已办（站点仓）**：① 中英结构同步守卫已在
   `agent-store-site` 重建（`scripts/check-docs-sync.mjs` + 单测，加 `check:docs-sync` /
   `test:docs-sync` 两个脚本），实跑 **9 页 0 drift**、`--self-test` **9/9**、单测 **16/16**；
   ② 站点文档 §7.3 已改为 **`46 / 68`** / 22 项（中英各一处），并补一条 MCP 读写面的宿主管理面
   说明。**两处相邻过期已按「协议面以源码为准」统一**：§3 的 `APP_SERVER_PROTOCOL_VERSION` 示例
   `2026-09-15` → **`2026-09-18`**（中英各一处），并在页面头部补一条**协议面口径**（§3 常量与
   §7.3 计数按仓库工作区取值、工作区领先于任何已发布版本、未发布差异指向 `upgrade` §8 与
   `changelog` §4）；顺带把 `changelog` §4 那张未发布表的「协议方法面增量」一行补上三个 MCP 方法
   与 `store/list` 的 `published_at`。**版本号口径未动**：工作区仍是 `0.1.0-beta.3`，与文档
   「本文与仓库当前对应 `0.1.0-beta.3`」一致——有张力的是 **wire 面**，不是版本号。

## 本轮（2026-09-21）

1. **新增连接器调用代理 `connector/call`**（规格：`05` §4.3.2；方案与验收：
   `24-external-agent-skill-and-mcp-access.zh.md` §5）。外部 agent 要用已装 MCP 连接器，
   缺的是**调用面**——连接参数与凭据协议刻意不给（`transport_summary` 是展示摘要、token
   永不跨界），于是改为**宿主持有连接与凭据、替调用方执行**。
   **指纹 `2026-09-20` → `2026-09-21`，新增一个方法**（WS + HTTP 各一侧）；方法计数守卫
   由 `47 / 23` 改为 **`48 / 23`**（`connector/call` 是 JSON 进出，走正常 HTTP 绑定）。
   **设计要点**：三道门（宿主 `[connector_proxy]` opt-in → 显式 allowlist → 连接器已启用），
   **默认全关**；`result` 逐字透传上游结果对象；工具级失败走 `is_error` 而非 wire 错误；
   调用方只能点名已注册的 `connector_id`（给 `url`/`command`/`headers` 一律 `invalid_request`，
   因此**不构成 SSRF**）；审计记连接器/工具/结果/字节数/耗时，**不记 arguments**；结果
   ≤1 MiB、超时 30s。**三种传输全部支持**（stdio / Streamable HTTP / SSE）——SSE 起初被
   我以「仓库没有 SSE 夹具」为由跳过，那条理由是**错的**（夹具就在
   `tests/connection_test_integration.rs`），随即实现并验证，订正记录见 `24` §9.1。
   **跨仓已同步**（`agent-store-site`）：`typescript-sdk.md` §2 常量、§7.3 计数 `48 / 23`、
   `connectors` 子客户端新增 `call`、错误码；`examples-sdk.md` 补用法；`changelog` §4 台账。
2. **补上一处遗漏**：阶段 1 引入的 `response_too_large`（技能文件面）当时**没有**登记进
   `10` §7 的公共错误码——我当时的判断是「没新增 `AppError` 变体所以不必改」，这混淆了
   **实现层枚举**与**wire 公共码**：`10` §7 管的是后者。本轮连同 `connector_call_timeout` /
   `connector_call_failed` 一起补齐，并写明「先判后读、拒绝而不截断」的语义。
3. **stdio 会话复用**（纯实现层：**无新增方法/字段/错误码，指纹不变，站点无需改**；规格：
   `05` §4.3.2 末段；记录：`24` §9.1）。阶段 2 把「按连接器 id 的空转会话池」列为延后项，
   本轮补上并**只做 stdio**——理由是**池化我们拥有的**：stdio 子进程是我们 spawn 的，而
   HTTP/SSE 的会话 id 由对端决定何时过期，缓存它等于用「稳定成功的调用」换「省一次往返」
   （`reqwest` 本就在底下复用 TCP/TLS）。三条不显然的规则：身份 = **连接器 id + 解析后凭据**
   （用 id 而非注册名，轮换凭据即换新会话）；**超时或管道断裂必须丢弃会话**（池化引入的新
   风险：管道里可能还留着上一次的答复，复用会让下一次调用归错因）；池满且都在忙时**退回
   一次性调用**。新增跨平台真 MCP stdio 夹具
   （`crates/backend/nomifun-mcp/tests/fixtures/fake_stdio_mcp.mjs`——既有 stdio 夹具是
   `#[cfg(unix)]`）与 12 条测试（该文件 13 → 25 条）；其中「杀进程」一条用**心跳文件**把
   「真的被杀」与「因 EOF 自己退出」区分开，且该尺子本身先被单独验证过。
   **真实链路读数**：两次真 `connector/call` 为 **586 ms → 3 ms**，夹具侧 `initialize` 只出现
   1 次、两条 `call` 同 pid、两次结果各自正确（没有归错因），`server.close()` 后无残留进程
   ——这条验的是**接线**（池在 router 构造期只建一次），单测覆盖不到。
4. **协议指纹形状变更：日期戳 → `fp-<n>` 计数器**（`2026-09-21` → **`fp-1`**）。**不改任何
   wire 行为**，但校验是严格相等，所以每个客户端都必须跟着更新。动机是日期戳会被误读：它
   **既不是变更日、也不是发布日期**，连续改动每次加一天、常超前于日历（改前是 `2026-09-21`，
   而当天是 `09-16`）。计数器保留了日期唯一的优点——**自排序**——同时不再像日期/像版本。
   落点仍 8 处（本仓）+ 2 处（站点）；`scripts/check-protocol-fingerprint.mjs` 的形状常量
   `FP_SHAPE` 一并改掉。**这一改必须赶在 `beta.4` 之前**：`2026-09-21` 尚未随任何版本发布，
   现在换零额外代价；发出去之后再换就是对真实用户的破坏性变更。

## 本轮（2026-09-20）

1. **新增技能文件读面 `skill/files` · `skill/file`**（方案与验收：`24-external-agent-skill-and-mcp-access.zh.md`
   §4；规格：`05` §4.3.1）。技能是**目录**（`SKILL.md` + `references/` / `scripts/` /
   `templates/` / `assets/`，`02` §5、`17` §5），而 `skill/get` 只回 ≤1200 字的正文摘要，
   附属文件此前**没有任何读面**——外部 agent 看到技能名也拿不到内容。
   **指纹 `2026-09-19` → `2026-09-20`，新增两个方法**（HTTP 各一条路由）。本仓落点 8 处
   代码/夹具；方法计数守卫由 `46 / 22` 改为 **`47 / 23`**（新增 `skill/files` 映射；
   `skill/file` 因 HTTP 侧回原始字节、非 JSON 信封，**刻意不进** JSON 传输的路由表，计入无绑定）。
   **跨仓已同步**（`agent-store-site`）：`typescript-sdk.md` §2 常量示例（中英各一处）、§7.3 计数
   `47 / 23`、`skill/*` 子客户端新增 `files` / `readFile`、`changelog` §4 未发布台账；
   `examples-sdk.md` 补「读技能附属文件」用法。
   **顺带订正两处既有偏差**：① `05` §4.8 的 mention 示例里 `skill` 用了**组件 id**
   （`wb-demo-release-notes`），而 `skill/list` 公布的是**技能名**——照抄会静默不挂载
   （`SkillId::parse` 降级为 `legacy:<组件id>` 后按名查不到），已改为技能名并补一节来源说明；
   ② `nomifun-importer/src/install.rs` 的 `materialize_skills_copies_under_managed_prefix`
   断言消息写着「only SKILL.md is copied」，与代码（`copy_dir_into` 递归）和它自己的断言相反，
   已改正。
2. **一处门禁盲区的实证**：`crates/backend/nomifun-app/tests/common/mod.rs` 的
   `build_app_with_skill_paths` 用 `AppConfig::default()`（相对 `work_dir`），而
   `create_router_with_states` 启动时构建 App Server workspace 注册表、**拒绝非绝对根**，
   因此该 helper **每次调用都会 panic**。本次新增的 `skill_files_e2e` 使它第一次被
   真正调用，遂修掉（改用绝对 `data_dir`/`work_dir`）。
   **订正（2026-09-21）**：当时我写的理由是「唯一使用它的 `tests/extension_e2e.rs`
   没有登记进 `Cargo.toml` 的 `[[test]]`（不参与编译）」——**这条是错的**。我当时只按
   `name` 搜了 `[[test]]`，漏了 `tests/suites/content.rs` 的 `grouped_tests!`：
   `extension_e2e.rs` 是以**模块**形式编进 `content_e2e_suite` 目标的，一直在编译、一直在跑，
   并在 **13 处**调用这个 helper——所以那处 panic 一直在让这 13 条测试失败，helper 修好后
   `extension_e2e` 实测 **49 passed / 0 failed**。`Cargo.toml` 里
   「a new top-level test file cannot be silently omitted」不是空话：`content.rs` 有一条
   `every_top_level_integration_test_is_registered` 门禁做集合相等断言。
   **同族缺陷已一并修掉（2026-09-21）**：`build_app_with_noop_opener`（`shell_e2e`）、
   `build_app_with_mock_version`（`system_version_e2e`）、`build_app_with_mock_agents`
   （`message_e2e`）也都传了相对 `work_dir`，全部改为绝对路径。读数：`shell_e2e` **27/27**、
   `system_version_e2e` **5/5**、`message_e2e` **38/38**。其中 `shell_e2e` 的 STT 两条是另一个
   原因（provider `base_url` 漏了 `/v1`，`st7` 因只断言 502 而一直空过）；`system_version_e2e`
   的 `full_system_flow_e2e` 用的是早已作废的 provider 请求体（`credentials`/`auth_scheme`/
   `initial_model`，DTO 是 `deny_unknown_fields`，故 400）。

## 本轮（2026-09-19）

1. **新增通知 `conversation/list-changed`**（`05` §12.3.1）：会话**列表**投影的变更（`created` /
   `updated` / `deleted`）此前只发宿主通道，App Server 侧看不见——于是**自动标题**（首条消息几秒
   后由服务端异步生成）与 `is_processing` 的翻转都到不了界面，侧栏一直停在客户端 `send()` 时的
   乐观快照上（名字空白、一直「正在处理」）。投影取自既有的 `conversation.listChanged`，**不设
   订阅门槛**（列表是全局的）、**不带 `sequence`**（不是转写帧，不参与缺口检测）、尽力而为
   （`conversation/list` 仍是权威）。指纹 `2026-09-18` → **`2026-09-19`**，**无方法增删**，方法
   计数守卫（46 映射 / 22 无 HTTP 绑定）不动。
   **落点**（按 `16` §8.1 记的「指纹落点比三处更广」全量同步）：`nomifun-app-server` 的
   `PROTOCOL_VERSION`、`web/packages/protocol` 的 `APP_SERVER_PROTOCOL_VERSION`、
   `web/packages/client/src/http-transport.ts`、`web/scripts/mock-server.ts`、`web/scripts/smoke.ts`
   （2 处）、`web/packages/sdk/src/readiness.test.ts`（2 处）。**跨仓已同步**（`agent-store-site`）：
   `typescript-sdk.md` §2 的常量示例 `"2026-09-18"` → `"2026-09-19"`（中英各一处；该页在站点仓已
   重编号为 §2 / §5.3，旧文的 §3 / §7.3 引用一并订正）；`ServerNotification` 的导出说明补上
   `conversation/list-changed`；§6.2 补一条**不变量**（该通知不带 `sequence`，不得推进 `lastSeen`、
   不参与缺口判定）；`changelog` §4 的未发布台账与 §2 的「协议面口径」注同步登记。复跑站点守卫：
   `check:docs-sync` **10 页 0 drift**、`--self-test` 9/9、`test:docs-sync` 16/16。
2. **客户端接线**（`web/src/store/appStore.ts`）：`connect()` 时挂接通知监听（换 client / 断开时
   解挂），`deleted` 走整份 `conversation/list` 重读（「读一行」表达不了「少一行」），其余走单行
   重读。**前一轮的兜底保留**：`turn.status` 非 running 时也重读该行——那条管 `is_processing`，
   这条管标题的**及时性**（实测标题在发消息后约 5s 就绪，回合 14s 才结束）。
5. **仓库级收尾**（`16` §8.6，均为用户点名）：① `ui/` 门禁退出 `bun run check`（**13 → 5 环**，
   脚本与登记保留）——聚合门禁此前红在**第 1 环** `typecheck`（`--filter=./ui`）且**从不检查
   `web/`**，改后 **exit 0**；链的描述在 `AGENTS.md` / `CONTRIBUTING.md` /
   `docs/contributing/development.zh.md` / `ui/.../MIGRATION.md` 同步改真，两个 README 的脚本表用
   `help --readme` 重生（顺带补上此前漂移掉的 `test:market` / `check:market` 两行）。② 清掉本会话
   自己造的两处孤儿（`.secondary-button.is-on` + 5 个 i18n 键，其中 4 个是我上轮漏报的）与指令
   要求的既有死代码（4 个 `*Categories` memo + `catalog.catAll`，确属早于本批，成因 `61d56f6bc`）
   ——zh/en 键现 **676 : 676 双向零差**。③ `web` 那条既有 typecheck 错（TS2741，用例缺
   `client: ClientInfo`）已按「补齐用例、不放宽已发布契约」修掉，`cd web && bun run typecheck` →
   **0 错误**。**仍未做**：把 `typecheck:web` / `test:web` 接进 `check`（`web/` 至今不在任何仓级
   门禁里，前置条件现已具备）。
