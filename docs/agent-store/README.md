# Agent Store 文档索引

> 最后核对：2026-09-17（**市场源改为 zip 单包托管（doc 30）**：`AppServerMarketplaceSourceKind` 新增 `zip`——一个归档、**归档根即市场根**；官方三个市场（experts/skills/connectors）从本站 `/source/<market>/…` 的逐文件托管迁到 ModelScope 归档，`experts` 首次获取由 14,714 个请求 / 611.3 MiB 降为 1 个请求 / 289.0 MiB，站点产物 22,706 → 约 742 个文件（只留 648 张目录页图标，`HOSTED_DEFAULT` 清空）；客户端用 `HEAD` 的 `X-Linked-Etag`（**内容 sha256**）判新旧并校验完整性、归档流式落盘且**不进入 staging**、解压走 `nomifun-common::zip_safe`（预算显式抬高：默认 256 MiB 装不下 611 MiB）；**指纹 bump 到 `fp-7`**（本仓 7 文件 10 处 + 站点 2 处，方法计数不变仍 `48 / 71`）；**顺带修掉两个真缺陷**——`looks_like_market` 漏了 `.codebuddy-plugin/marketplace.json`（官方 experts 市场根**只有**这一个清单，该布局的 github/git/zip 远程源一律被判「不像市场」），`plugin_marketplaces.source_kind` 的 CHECK 未含新值（迁移 `059` 重建表，带上 056/057 新增列）；`lib.rs` 里把未知 kind 静默当成 `url` 的 `_ =>` 兜底改为显式 `parse` + 跳过并告警；老用户 `config.toml` 迁移**只写文档不自动改写**；**三个归档已上传 ModelScope 并通过远端摘要回验**（`--verify-only` 三绿），客户端对真实归档的端到端见 `30` §9.6）；2026-09-15（**安装面五动词「做真事」：`install/run` 可重入（同快照不产生第二个 Preset）、`uninstall` 真正释放产物（skill 目录 / Preset / `mcp_servers` 行，已不在算成功、失败保留 `installed=1`）、`disable`/`enable` 真正移动运行时状态（skill 例外＝目录标记 `skill_disable_flag_only`）、新增结构化 `outcomes`（`action` 八值 + 稳定 `code` 闭集）、`store/install-entry` 版本感知（升级只有「卸载再安装」一条路）、client 新增 `store` 子客户端（20 例）、宿主管理面类型搬进 `protocol`、商店假「更新」控件已摘除；**协议指纹 bump 到 `2026-09-15`**（现有 DTO 加字段，8 处代码/夹具 + 2 处站点文档，方法计数不变）**；同日：**`agent/run` 加稳定码 `preset_disabled`、`team/run` 加 `agent_disabled`（点名成员，检查在 `resolve_team_members` 每次运行、先于 Connector 栅栏）**；2026-09-14：**`20` §9.5 + `16` §5.3：`config/get.mcp` 新增 `adopted`（宿主是否真的把这批声明注入会话——`servers` 描述文件、`adopted` 描述宿主），设置页据此分出「使用中 / 未使用 / 无法判断」三态；**协议指纹 bump 到 `2026-09-14`**（8 处代码/夹具 + 2 处站点文档，方法计数不变）**；同日：**`16` R16 追记 · 消息渲染：设置页三个错误字段改判别式联合 `ConfigMessage`，宿主散文不再过 `t()`（i18next 的 `looksLikeObjectPath` 会把 `mcp.json is not valid JSON: …` 截成冒号后半段）**；2026-09-13：**`16` R16 追记 + `20` §9.4：设置页新增只读 `mcp` 分区渲染 `config/get.mcp`（nav 三→四；该批零协议变更）；同日上一轮：`20` §7.9.1–§7.9.3/§9.4 补齐参考实现文档里的全部可选字段（`cwd` / `bearerTokenEnvVar` / `startupTimeoutMs` / `enabledTools` / `disabledTools`）并把 `headers` 的 `secret:NAME` 语义在三条装配路径收敛为一个函数；server 级工具过滤在注册**之前**裁剪；**无协议变更**，指纹保持 `2026-09-13`**；再上一轮 2026-09-12：**`21` D14 + `20` §7.9/§9.3：Agent Store 支持 Kimi 式 `~/.agent-store/mcp.json` 声明 MCP（用户级、纯内存注入、不投影进 `mcp_servers`），协议指纹 bump 到 `2026-09-13`**；更早 2026-09-11：新增 `22-webui-productionization.zh.md`；`16` C 档二次复核改判；**版本框架订正：发版前只有一个版本，统一称 v1，见 `16` §7 决策 4**；引用/口径统一 + 证据与 `TC-*` 正文并入编号文档；**`20` 重构：工具面表达层定为 `~/.agent-store/config.toml [tools]`，新增 `16` §7 决策 5**；**`20` Step 7 全部落地：`team/run` + Team 层委派放行，协议指纹 bump 到 `2026-09-12`**）
> 迁移编号调整（2026-09-18 与 `main` 合并）：本目录文档正文里的 **Agent Store 侧迁移短号**按合并前编号书写；合并后整体后移 6 位，以免与 `main` 的 `048`–`053` 撞号——`048→054`、`049→055`、`050→056`、`051→057`、`052→058`、`053→059`、`054→060`、`055→061`、`056→062`、`057→063`、`058→064`、`059→065`（`main` 的 `048`–`053` 保持不变）。带 `.sql` 的文件名已按新编号更新；短号保留原样，以免改写当时的实测记录。
> 用途：本目录文档的地图、权威顺序与状态图例。新会话/新成员先读本文，再按需深入。
> 维护约定：新增文档编号顺延（当前到 `31`）；状态变更时同步更新本索引与文档头部“更新”行；公共契约变更先写 `16-sdk-webui-site-priority-plan.zh.md` §7（决策记录），再改基线文档。

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
| `25-release-runbook.zh.md` | **发布操作手册（本文）**：一次发布的两个出口（npm 四包 / 站点仓 GitHub Release）、7 条不变量及其机械判据、有序清单 S0–S8、**站点文档同步清单**、失败与回退、已知缺口 | ✅ 现行（每次发版照做；不含产品准入判定） |
| `28-webui-composer-connector-switch-plan.zh.md` | **WebUI 输入区**：连接器退出 `@` 提及、`+` 菜单与连接器抽屉改为真正的启用开关（宿主级 `enabled`，走第一方 `POST /api/mcp/servers/:id/toggle`，零 wire 变更）；行内「连接」＝发起 OAuth；含语义边界、验收与偏差 | ✅ 已落地（2026-09-23；4 处实现期偏差见 §9.1，手测项待用户验证） |
| `27-conversation-binding-plan.zh.md` | **会话绑定**：每轮技能（`conversation/send` 收 `mentions`）+ 会话级专家 / 专家团（`conversation/create` 收互斥的 `agent_id` / `team_id`）；含**不做每轮连接器**的理由与将来的两条路、验收口径、指纹与跨仓步骤 | ✅ 阶段 1 / 2a / 2b 均已落地（`fp-3`/`fp-4`/`fp-5`）；仅剩「Leader 首轮委派」真机实测 |
| `30-market-zip-hosting.zh.md` | **市场 zip 单包托管**：`AppServerMarketplaceSourceKind` 新增 `zip`（一个归档、归档根即市场根），官方三个市场从本站逐文件托管迁到 ModelScope 归档；含站点打包/发布脚本口径、`X-Linked-Etag`＝内容 sha256 的新鲜度与完整性设计、老用户配置迁移（只写文档）、以及实现期发现的两个真缺陷 | ✅ 已落地（2026-09-17，`fp-7`）；三个归档已上传并通过远端摘要回验，客户端对真实归档的端到端见 §9.6；站点产物 22,706 → 742 |
| `29-send-model-and-effort-plan.zh.md` | **随调用指定模型与思考等级**：`conversation/send` 与 `agent/run` 各收可选的 `model` / `reasoning_effort`；口径＝**粘性（从本轮起生效）**，`agent/run` 的等级挂 `ResolvedPresetSnapshot.reasoning_effort`（免迁移）；含顺序论证、指纹 `fp-6` 落点、团队一侧的边界 | ✅ 已落地（2026-09-23，`fp-6`）；门禁与真机 `SM-001`–`SM-010` 读数见 §10.1 |
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
| `31-sdk-entry-shape.zh.md` | **SDK 入口的形状与命名**：`launchClient()` 名不副实（返回的不是 client）、`client` 一词三义、调用点恒多一跳；与 OpenAI Codex（Python）/ Moonshot Kimi Code（Node）两个外部实现逐条对照；含 3 处真问题、2 处「看着像但不是」、3 个候选方案、15 + 13 个文件的实测影响面，以及「**非 wire 变更 ⇒ 不 bump 指纹**」的边界 | ✅ **已实现**（2026-09-18：方案 B 落地 + §10 改名为 **`launchHarness` / `Harness`**；读数与两个实现期约束见 §9/§10。**尚未发版**，已发布的最新版仍是 `beta.5` 的旧形状） |

## 阅读顺序建议

- **接手开发**：本索引 → `16`（当前执行顺序与卡点处置）→ `15`（当前顺序）→ `05`/`07`（协议与 SDK 基线）→ `13`（P0 细则）→ 证据页
- **了解架构**：`00` → `01` → `04` → `05` → `10`
- **对接 SDK**：`12` → `07` → `05`
- **生产化评估 / 安全加固**：`22`（WebUI 生产化立项）→ `11`（差距清单）→ `06`（Connector OAuth 与安全模型）
- **发布评估**：`09` → `agent-store-v1-test-cases.md`
- **执行发版**：`25`（操作手册）→ `12` §6（发行形态）→ 站点仓 `docs/release-process.md`（站点半边）

## 已知待修（P1）—— 全部收口（2026-09-11）

1. **冻结文档状态行**：✅ 已订正（2026-09-11）——`00/01/04/06/08` 头部与 `15`／`16` §6 的「架构/计划/路线冻结」改为「基线／现行」（发版前非冻结，见 `16` §7 决策 4）；文档日期快照仍以本索引为准。
2. **App Server stdio 口径**：✅ 已统一（2026-09-11）——`00/05/06/07/08/10` 的 App Server 传输表述均标注「V1 不含 stdio」（`16` §7 决策 2）；MCP STDIO 连接器（`06` §7）不受影响。
3. **证据文件位置**：✅ 已定（2026-09-11）——证据正文已整体并入编号文档（`02` §14、`06` §12/§13/§14、`13` §14/§15、`15` §10），本索引只登记并入后的位置；**7 份独立原件已删除**，引用同步改指并入位置（含 3 处 Rust 源码注释与 `roadmap`）。
4. **规划文档重叠**：✅ 已定（2026-09-11）——`开发计划.md`/`技术方案.md`/`roadmap` 均保留为历史追溯、正文不再更新，现行内容以 `16` §6／§7、`13`、`15` 为准（各文件头部已有指针/合并标注）。
5. **TC 正文归属**：✅ 已收敛（2026-09-11）——`TC-*` 逐条正文归各归属文档（`02` §13、`13` §10、`05` §14、`06` §11、`19` §9），`agent-store-v1-test-cases.md` 精简为「§1/§2 公共口径 + §3 归属总表」；`13` §11 的环境与等级口径收敛为指向 §1/§2 的指针（消除重复副本）。
6. **「冻结」措辞收尾**：✅ 已收敛（2026-09-11）——除语义性「快照冻结」（TC-RT-002 / ResolvedPresetSnapshot 等）外，`00` §10、`10`、`12`、`13` 及 `16` 任务表的「公共契约冻结／冻结文档／v1 冻结」统一改为「基线／现行正文」；`开发计划.md`／`技术方案.md`／`roadmap` 为历史文档，原文保留（头部已标注）。

## 本轮（2026-09-18 发布 `0.1.0-beta.6`）

**第三次走完整发布链**，也是第一次**发版前先做文档事实核验**：

1. **npm**：`protocol` / `client` / `runtime-win32-x64` / `sdk` 四包同为 `0.1.0-beta.6`，dist-tag `beta`；`latest` 仍指 `0.1.0-beta.2`（按手册**有意不动**）。包内顺序 protocol → client → runtime → sdk。注册表 `time`（sdk 口径）`2026-09-18T11:08:21.813Z`；另三包 protocol `11:06:44.679Z` / client `11:06:28.854Z` / runtime `11:08:13.963Z`。**发布刚结束时 canonical packument 仍是旧值、tarball 甚至假 404，约 2 分钟后转正**——与手册 §1 的告警一致。
2. **同一个 exe 服务两个出口**：`target/release/agent-store.exe`（`--features static-webui`，构建 **25m29s**，`sha256 823A9492…DB7DB146`）经两个出口发布——npm runtime 包里**从干净目录 `npm i` 装回来复核，包内 exe 与本地构建逐字节相同**；站点 Release 资产 zip 76,169,002 B，GitHub 服务端 `digest` 与本地 `SHA256SUMS.txt` 一致。
3. **本版唯一破坏性变更是 SDK 入口**（方案与取舍：`31` §5 方案 B + §10）：`launchClient` → **`launchHarness`**、`LaunchedClient` → **`Harness`**、`LaunchOptions` → **`HarnessOptions`**，返回值不再有 `.client` 一跳，`initializeResult` → **`handshake`**，`close()` 语义变强。**wire 面未动**——指纹仍 `fp-7`、方法计数仍 `48 / 71`，所以这是**纯 TS 侧**破坏性变更，不触发指纹流程；发布前 `verify-published-sdk` 对自建 exe 报 `VERIFY-OK { fp-7, 1.4.3, models: 5, storeItems: 0 }`。
4. **发版前修正站点 API 参考 13 处事实错误**（子代理审计 + 逐条对源码核验，全部属实）：`7 个子客户端`→**9**、protocol「无任何运行时代码」、`request_id`→**`requestId`**、`readiness.url` 被当成 API 根、`catchUp()` 被当成手动追平、市场「整棵树镜像 / 约 90 秒」、`requestTimeoutMs` 的错误建议、`experimental` 标记不存在、`ReasoningEffort` 缺 `max`、§5.2 缺 `runs.plan` / `connectors.call` / `skills.files·readFile`、就绪超时消息缺 `after ${timeoutMs}ms`、`sequence` 是**连接本地**、`initialize`/`initialized` 也有 HTTP 路由。另**自行扫出 6 处同源过时说法**（`examples-sdk` §3.3/§12/§14 的"首个 `store/list` 会镜像市场树、要放宽超时"与 `configuration.md` 的"整棵树"，`plugins-market.md` §48 说的是第三方 `url` 源的逐文件镜像，**正确，不动**）。
5. **站点同步**：`changelog` 新增 §2.1（破坏性 + 升级影响），JSON 块与版本表按注册表输出逐字更新，旧 §2.1–§2.5 顺延为 §2.2–§2.6 并把交叉引用同步，§4 台账清零；`upgrade` §2 版本表、§3 dist-tag/JSON、**新增 §6.5**（`beta.5` → `beta.6` 迁移）、§8 清零；`typescript-sdk` §1 版本状态。站点提交 `62dacb26` → 部署 `dpasuwboeo01`（`Success` / `UsedInProd`）。
6. **两处实测偏差已登记进手册 §7**（第 7、8 条）：① **S1 基于工作树构建**——工作树里别人未提交的 546 行 provider 改动（`agent_store.rs` / `lib.rs` / `routes.rs`）被烧进了本版 exe（发布基线 `219383745`），用户知情后选择如此，**不是默认做法**；② **`release:check` 第 1 环（`ui/` typecheck）恒红**（73 个既有错误 / 28 个文件，全在已提交代码里），所以 S4 只能在**逐环**层面成立。
7. **自检读数**：`check:fingerprint` / `check:release-sync`（`beta.6` 四 manifest + 8 pin + 站点 release.json）/ `web typecheck` / `web test`（513 passed, 1 skipped）/ `cargo test -p nomifun-app-server`（**165 passed**）/ 站点 `release:check:site` **全绿**；`release:check` **EXIT=2**（见第 6 条②）。**部署后线上自检**：两语言文档页是真内容（非 SPA 空壳）且 `launchHarness` 与 `beta.6` 已上线、`session.client` 残留清零、zip 直链 200、三个市场归档摘要回验全绿、站点产物 742 文件（上限 20,000）。

## 本轮（2026-09-17 发布 `0.1.0-beta.5`）

**第二次走完整发布链**，且是第一次**一口气清掉积压的多次指纹递增**：

1. **npm**：`protocol` / `client` / `runtime-win32-x64` / `sdk` 四包同为 `0.1.0-beta.5`，dist-tag `beta`；`latest` 仍指 `0.1.0-beta.2`（按手册**有意不动**）。包内顺序 protocol → client → runtime → sdk。注册表发布时间 `2026-09-17T11:41:10.171Z`。
2. **同一个 exe 服务两个出口**：`target/release/agent-store.exe`（`--features static-webui`，`sha256 2b1c099c…`）既进 npm runtime 包（tarball 72.3 MB），又以**同一哈希**经 `release:pack --expect-sha256` 打进站点 Release 资产（zip 69.2 MB / 72,510,453 B，`release:publish` 的下载回验 sha256 一致）。
3. **本版一次发布了 `fp-1` → `fp-7` 六次递增**（`beta.4` 之后积累的全部）：连接器工具的 `input_schema` / `tools_truncated`、按轮挂载技能、`agent_id` / `team_id`、`model` / `reasoning_effort`、市场源 `zip`。**无方法增删**，`48 / 71` 不变。
4. **`beta.5` 是破坏性发布**，判据是**指纹严格相等**而不是类型收窄：`beta.4` 的客户端连不上本版运行时。与已发布产物对读可证**不需要改代码**——两版 `index.d.mts` 的导出名都是 **141 个**，一个不多一个不少，新增的只有可选字段。
5. **订正上一版的一处台账**：`MentionKind` / `MentionRef` 与两处 `mentions` **在 `beta.4` 的已发布声明里就已经存在**，所以 `beta.5` 的条目把它写成宿主侧**语义**变化，不声称是新增字段。更正记在站点 `changelog` §4。
6. **发布前修掉一个会让「配置文件声明 zip 市场源」静默变空店的缺陷**（`30` §9.8）：`AgentStoreMarketplace::resolved()` 自带第二份 kind 白名单，不认识 `zip`，导致 config 源被 `filter_map` 丢掉而 `complete` 仍报真；`agent-store init` 的向导产物同样中招。已删白名单、把判定权收归 `AppServerMarketplaceSourceKind::parse`，补三个回归钉。
7. **手册新增一条实测坑**（`25` §3 S5）：发布步骤会把 181 MiB 二进制写进 `web/packages/runtime/vendor/`，Windows 写入期锁文件，Vite 的 watcher 会以 `EBUSY` 退出——已把该目录加进 `web/vite.config.ts` 的 `watch.ignored`。
8. **跨仓站点文档同步**：`changelog` 新增 §2.1 并把 §4 未发布台账清空；`upgrade` 新增 §6.4（`beta.4` → `beta.5`）、§8 改为「无未发布差异」并补 `beta.4` → `beta.5` 的对读示例；`typescript-sdk` §1 的版本状态改为 `0.1.0-beta.5`。`content/release.json` 同值。
9. **本仓改动**：四个 `package.json` 的版本与 sdk 的依赖 pin + `web/vite.config.ts` 的 watcher 排除 + 上述缺陷修复 + `25` 的坑记录。

## 本轮（2026-09-16 发布 `0.1.0-beta.4`）

**首次按 `25-release-runbook.zh.md` 走完整条发布链**（npm 四包 + 站点仓 GitHub Release + 站点上线），并把当次实测出的坑写回了手册：

1. **npm**：`protocol` / `client` / `runtime-win32-x64` / `sdk` 四包同为 `0.1.0-beta.4`，dist-tag `beta`；`latest` 仍指 `0.1.0-beta.2`（按手册**有意不动**）。包内顺序 protocol → client → runtime → sdk。
2. **同一个 exe 服务两个出口**：`target/release/agent-store.exe`（构建带 `--features static-webui`，`sha256 fbe3d892…`）既进 npm runtime 包（tarball 72.3 MB），又以**同一哈希**经 `release:pack --expect-sha256` 打进站点 Release 资产（zip 69.1 MB，服务端 `digest` 与本地字节一致）。
3. **跨仓站点文档同步**：`changelog` 新增 §2.1 条目并把 §4 未发布台账清空；`upgrade` 新增 §6.3（`beta.3` → `beta.4` 的破坏性升级步骤）、§8 改为「已发布产物的差异与自查方法」；`typescript-sdk` §1 的协议面口径改为「自 `beta.4` 起与已发布产物一致」。**`48 / 71` 与 `fp-1` 已用已发布产物实测复核**（`httpRouteTable()` 键数 = 48；声明里 event_type 的 `| string` 兜底消失、`APP_SERVER_PROTOCOL_VERSION = "fp-1"`）。
4. **口径修订（用户决定）**：站点 `changelog` §3 与 `upgrade` §1/§9 的「破坏性变更走 minor 号」改为「beta 线内随下一个预发布序号发布，minor 号留给退出 beta 之后」——原口径与 `0.1.0-beta.4` 实际承载的破坏性变更（`ConversationEvent.event_type` 收窄 + 严格相等的指纹）相矛盾；修订记录写在站点 `changelog` §3。
5. **手册新增三条实测坑**（`25` §1 / §3 S5·S6 / §7）：npm 读取侧 CDN 滞后（发布成功后 `npm view` 可能给旧值甚至假 404，复核要用 canonical packument URL 且**不要**加查询串）；GitHub Release 的慢点在**下载回验**而非上传，国内网络需 `HTTPS_PROXY`；npm 2FA 账号必须用「带 bypass 2FA 的 granular token」，否则在第一个包就以 `E403` 中止（注册表未被改动）。
6. **本仓改动**：四个 `package.json` 的版本与 sdk 的依赖 pin（由 `publish-packages.ts` 按 `VERSION` 重写）+ `25` 的上述三条坑记录。

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

## 本轮（2026-09-23）

1. **每轮技能（`fp-2` → `fp-3`）**（方案：`27-conversation-binding-plan.zh.md` 阶段 1；规格：`05` §12.3）。
   `conversation/send` 新增可选的 `mentions`（**现有 DTO 加字段**），**只认 `kind: "skill"`**：技能是
   每轮载荷（正文与不可变快照随这一轮走），而会话的技能/MCP/预设快照在 create 之后只读，所以「一轮挂
   技能」不需要、也不允许改写那个快照。`agent` / `connector` 两类**显式 `invalid_request`**——专家是
   会话身份、连接器是宿主级开关，两者在 `send` 上都没有载体；拒绝而不是静默忽略，否则调用方会以为挂上了。
   失败仍早于占用幂等键。**无方法增删**，计数守卫仍是 `48 / 71`。
2. **以专家开场（`fp-3` → `fp-4`）**（方案：`27` 阶段 2a；规格：`05` §12.2）。`conversation/create`
   新增可选的 `agent_id`：解析走 `agent/run` 的同一套语义（`agent_not_installed` / `preset_disabled` /
   来源白名单 / `runtime_unavailable`），然后把该专家的 **preset 快照**与**它自己声明的技能、连接器**
   一并冻进这一行。两个实现要点：**技能是「已解析快照」的可信通道**（`create_from_preset_snapshot`），
   而**宿主 auto-inject 的排除由会话层补**——agent-store 装出来的 preset 这一项是空的
   （`app_server_installer` 写死 `vec![]`），不补就会让宿主自动技能漏进会话。连接器停用 ⇒
   `connector_unavailable`。**换专家 = 新建会话**（PATCH 拒绝 preset/技能/MCP 三类键）。仍无方法增删。
3. **以专家团开场（`fp-4` → `fp-5`）**（方案：`27` 阶段 2b；规格：`05` §12.2）。`conversation/create`
   新增可选的 `team_id`，与 `agent_id` **互斥**。实现方式是**抽取**而不是复制：`team/run` 的前三步
   （成员校验 → 连接器栅栏 → 物化/复用执行模板 → 建 Leader 会话）成了
   `team_run::prepare_team_leader_conversation`，两个入口共用它，唯一区别是 `create` **不发 `goal` 首轮**——
   客户端自己的第一条 `conversation/send` 就是 Leader 的首轮。抽取的判据是 `team/run` 行为与错误码
   逐字不变（`execute_team_run` 只接管「发首轮 + 反查 execution」）。仍无方法增删。
4. **WebUI 侧同批收口**（方案：`28-webui-composer-connector-switch-plan.zh.md`）：连接器退出 `@` 提及，
   改在 `+` 菜单与连接器抽屉里以**启用开关**呈现（宿主级 `enabled`，走既有第一方
   `POST /api/mcp/servers/:id/toggle`，零 wire 变更）；行内「连接」＝发起 OAuth；次行改用 `status` 文案。
   聊天发送路径终于把 skill mention 真正传下去（此前 mention 在聊天路径上被丢掉）。
5. **站点同步**：`typescript-sdk` 两语言的 `send()` 签名补第 4 个参数（既有 `attachments` 与新增
   `mentions`，顺带补上此前缺失的附件文档）、`create()` 补 `agentId` / `teamId`、`examples-sdk` §7 增三条配方、
   `changelog` §4 未发布台账叠加 `fp-3` / `fp-4` / `fp-5` 三条。
6. **已知偏差（登记）**：站点正文在我动手前已把常量写成 `fp-3`（提前占名），本轮把它的**含义**补齐为
   「每轮技能」；`fp-3`/`fp-4`/`fp-5` 是代码侧真正落地的值（代码此前仍是 `fp-2`）。
7. **真机实测（阶段 2b）**：脚本 `web/scripts/sdk-live-team-leader.ts` + 含本次改动的 debug 二进制。
   官方默认市场**没有专家团**（实测 262 skill + 228 connector，`team = 0`），故用仓库夹具
   `software-company`（`import/run` + `install/run`，0 warning / 0 error）。读数：`create({ teamId })`
   成功建出 Leader 会话；客户端首轮被受理并真的开始跑；明确点名时 Leader 会 `ToolSearch` 找到
   `nomi_delegate` 并调用（拿到 `execution_id`）。**自然语言下是否自发委派由模型决定**（6 次里 2 次委派成功，
   4 次自己动手做）；同条件 `team/run` 也会 `team_run_not_started` ⇒ 2b 无需回退，但**不能**假定"发首轮就会委派"。
   另有两条真机契约：夹具团的连接器默认 `enabled=false`，`create` 会（正确地）以
   `connector_unavailable` 拒绝；同一会话上一轮未结束时不能再委派（`Conflict: … unfinished Agent Execution`）。
   逐条读数与判定口径的两处修正见 `27` §9.1。
8. **修掉一个既有缺陷：`agent/run` 的默认模型回退口径不一致**（真机发现）。它此前**只**读宿主 DB 的
   provider 注册表，而 config.toml 里的 provider 是**按需注册**的（其它路径解析模型时才写库），于是
   「全新宿主、还没解析过任何模型」时 `@专家`（`agent/run`）会以 `invalid_request`
   （`resolved_model is required`）失败——真机复现。现在 `default_run_model` **先**取 config 的
   `default_model`（与会话创建 / `team/run` 同源，`ensure_agent_store_provider` 按需注册），取不到才回退
   注册表；没有 config 文件的宿主行为不变。**运行期解析口径的修正，不动 DTO/字段，故不 bump 指纹。**
   真机验收：`web/scripts/sdk-live-mention-agent.ts` 的 `MA-001.fresh-host-agent-run` 在全新宿主上
   **PASS**（第一次调用即拿到 `run_id`），同批还钉住「未知 id ⇒ `not_found`」与「团 mention ⇒
   `invalid_request`（wire 上没有 team 这个 kind）」。
9. **仍未做**：WebUI 尚无「以专家 / 以专家团开会话」的入口（本轮只做协议面与 SDK）。
10. **随调用指定模型与思考等级（`fp-5` → `fp-6`）**（方案：`29-send-model-and-effort-plan.zh.md`；
    规格：`05` §12.1 / §12.4 / §5.2）。`conversation/send` 与 `agent/run` 各新增可选的 `model` /
    `reasoning_effort`，`ConversationView` 新增 `reasoning_effort`。**口径是「从本轮起生效（粘性）」**，
    不是「只影响这一轮」：运行时按会话行构建（换模型立即重建、换等级在下一个 turn 边界重建），
    所以 `send` 上带的值写进会话行、本轮就是新设置的第一轮；真·一次性需要引擎级的每轮模型通道。
    `send` 的三条顺序是契约：**忙判定（`running` / `is_processing` ⇒ `conflict`）→ 差异判定
    （全相同则不写库、不广播）→ 复用 `conversation/update`**（不新开第二条写模型的路）。之所以要
    忙判定在前：`update` 换模型会**立即拆运行时**且不看有没有在跑的 turn，而 send 的准入随后会以
    `Conflict` 拒绝——先落库再被拒就是"既丢回合又丢消息"。`agent/run` 一侧的 `model` 优先级是
    **显式 > preset 自带 > 宿主默认**（复用 `with_model`，即原 `with_default_model`，改名是因为
    显式调用点让"默认"这个名字说谎），`reasoning_effort` 走**免迁移**的载体
    `ResolvedPresetSnapshot.reasoning_effort`（参与者行的快照本来就是 JSON 列，与 `resolved_model`
    同位同性质；preset 解析永不设置它，故既有行/既有生产者的序列化逐字不变），由 attempt runner
    投影进尝试会话的 `extra`，此后与普通会话走**同一条**运行时读取路径。`AgentRunRequest` 会被幂等
    指纹序列化，所以两个新字段带 `skip_serializing_if`——缺席即不进指纹，纯升级不会让既有收据失配。
    **无方法增删**，计数守卫仍是 `48 / 71`。测试：`cargo test -p nomifun-app-server` **158 passed**
    （+5 条新增：wire 同形、差异判定、等级唯一读取口径、`agent/run` 新字段与指纹稳定性）。
11. **站点同步（`fp-6`）**：`typescript-sdk` 两语言 §2 常量、`send()` 的 `model` / `reasoningEffort`
    与「粘性、且从本轮起生效」的说明、`runs.agent()` 的显式模型/等级与优先级、`changelog` §4 台账
    叠加 `fp-5` → `fp-6`；`check:docs-sync` 报 0 drift。
12. **真机验收（`web/scripts/sdk-live-send-model.ts`）**：读数见 `29` §10.1。
13. **仍未做（登记为独立一轮）**：WebUI 选择器仍是「立即 `conversation/update`」。**不顺手改**的原因
    写进了 `29` §9.4：`selectedModelKey` / `selectedEffort` 是**全局** localStorage 设置而非每会话，
    改成随 send 携带会**静默改写用户没瞄准的会话**的模型；正确做法需要三个前置（读回字段[本轮已加]、
    选择器改每会话、只在必要时挂到 send），故单列一轮。

## 本轮（2026-09-22）

1. **连接器工具签名读面 + 授权单位上移（`fp-1` → `fp-2`）**（方案：
   `26-connector-schema-and-grant-policy.zh.md`；规格：`05` §4.3.2 / §4.3.3）。两件事其实是一件：
   **要人同意一个工具，就得让他看得见这个工具收什么参数。**
   1. **`ConnectorTool.input_schema`**（上游 `tools/list` 的 `inputSchema` 逐字）。宿主**早已**解析并
      随探针落库（`McpToolResponse.input_schema`），缺的只是映射；字段挂在既有 DTO 上，所以
      `connector/get`（自上次探针的缓存）与 `connector/test`（现场探针并落库）两条读面**同时**生效，
      **不新增方法**，计数守卫仍是 `48 / 23`。体积预算 `MAX_CONNECTOR_TOOLS_BYTES` = 1 MiB：
      `name` / `description` **永不省略**，放不下的 schema **整份**省略并置 `tools_truncated`
      ——**绝不截半个 JSON Schema**。这里刻意**不复用** `response_too_large`：该码管的是调用方会
      parse 并相信的**载荷**被截断，而目录面缺的是**显式标记的缺席**。
   2. **`[connector_proxy]` 的授权单位由「逐个工具」改为「连接器」**：`enabled` 为真即默认可调，
      `allow` 变**可选收窄**、新增 `deny` 为**可选减法**（在 `allow` 之后应用，与 `[tools]` 同序），
      条目词汇与 `[tools]` 统一为 `mcp__<连接器>__<工具>`（只有 `mcp__` 条目是 glob，`<连接器>` 可写
      注册名或 id）。**表这一层仍 fail-closed**（缺表 / 缺 `enabled` = 关），且 `allow` 的
      **缺席（全放）**与**空表（全不放）**刻意不合并。旧写法 `<连接器>__<工具>` 在新词汇下不再命中
      → 收窄到零，是 fail-closed 的方向；宿主启动时就它、以及「开了代理却没写任何名单」各给一条 warn
      （`ConnectorProxyPolicy::warnings`，纯函数，照 `NomiToolPolicy::syntax_warnings` 的先例）。
      匹配器与引擎**共用同一个 `glob` crate**，并照抄引擎 `registry.rs` 的用例表，防止两侧语义漂移。
   3. **代价写进正文，不留在代码注释里**：默认全放之后，「第三方可调」与 `enabled`（本机 agent
      会话可用）不再分离，而 UI 的「导入本机 agent 的 MCP 配置」会**自动 enable**
      （`mcpImportUtils.ts:144` + `useMcpServerCRUD.ts:76`）——即**导入即授权第三方**。这是本轮
      最实的代价，登记在 `26` §4.5；可审计的抓手是 `connector/list` 已带的 `enabled` 与新增的 `deny`。
   4. **命名决定：`connectors.test()` 不改名**（`26` §6）。本仓的分工是 `test` = 动作
      （wire `connector/test`、`test_connection`）、`probe` = 产物（`ConnectorProbeResult`、
      `probe_status`）；只改客户端那一半会得到 `probe()` → `ProbeResult` 这种动词名词同词、且与
      wire / 路由 / Rust 全都不一致的形状。真要改就**连 wire 一起改**——本次指纹无论如何都要 bump，
      所以那时的兼容性代价为零。备选与落点登记在 `26` §10，不在本轮做。
   5. **门禁读数**：`bun run check:fingerprint` → `✓ "fp-2" consistent across **10** landing point(s)
      in **7** file(s) here and **2** file(s) in the docs site`；两侧都只是 DTO 加字段，
      **无方法增删**，故站点的方法计数（`48 / 71`）与路由守卫（`48 / 23`）**都不用动**。
      **顺带补上一个门禁盲区**：`scripts/probe-agent-store-runtime.mjs` 此前停在 `2026-09-14`
      ——**跨了两次形状都没人发现**，因为没有任何东西指向它；本轮把它的值改为 `fp-2` **并把它列进
      `MIRRORS`**，此后 bump 会在这里响亮地失败，而不是留下一个连不上的探针脚本。
   6. **顺带订正两处会撒谎的注释**：`services.rs` 里「`allow` 为空也一律拒绝」与
      `connectors.ts` 里「`policy_denied` 是默认状态」——两句在改动后都变成假话，已按新语义改写
      （这类注释在本仓是承重的，不是装饰）。
   7. **补上方案里那条真实性闸门**（`connector_tools_e2e`）：真组合根 + **真 MCP server**
      （跨平台 stdio 夹具，由宿主自己 spawn）→ 注册 → app-server 握手 →
      `POST /api/app-server/connectors/{id}/test`，断言 schema **与夹具声明逐字相等**、
      **没声明 schema 的工具不凭空长出一个**（只断言「有值」的实现也能过，所以这条是必须的）、
      `tools_truncated === false`，且读面在连接器**未启用**时同样工作。为此外加夹具的
      `tools/list`——它此前只答 `initialize` 与 `tools/call`，**根本没法做成功探针**。
      读数：新 e2e **1 passed**；夹具既有使用者 `connection_test_integration` **25 passed**
      （加 `tools/list` 零回归）；登记门禁 1 passed。站点 `check:docs-sync` 仍 **10 页 0 drift**，
      `examples-sdk` 新增 §8.1「先读签名，再调用」（中英同构）。

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

## 追补（2026-09-18）· 默认市场按需下载

**背景**：`30` §11。doc 30 把「全量镜像」变成「一次请求」后，三包合计仍是 **324.2 MiB**
（`experts` 289.6 / `skills` 17.9 / `connectors` 16.7，用 1 字节 range 读 `Content-Range` 实测；
ModelScope 的 `HEAD` 只给 `X-Linked-Etag`，不给 `Content-Length`），而 `warm_default_marketplaces`
在**路由构造时**就调（D-SDK-1 ①），于是全新安装、无配置的机器**启动过程中**就拉完 324 MiB ——
即使用户从没打开过商店。用户拍板：**默认不下载，点了才下**。

1. **注册与取包拆开**（`nomifun-app` / `nomifun-app-server`）：新增内部 seam
   `MarketplaceProvider::register_unfetched`（占位行：`entries=[]`、`resolved_revision=NULL`、
   `auto_update=0`），`ensure_default_marketplaces` 的源选择抽成纯函数 `default_marketplace_plan()`：
   **配置里显式声明**的源照旧「注册 + 下载」（写进配置就是明确要求，也是天然 opt-in 开关，**不新增
   配置键**），**内置兜底**三源只注册不下载。`auto_update` 刻意不取 `is_official_source()`——否则任何
   声明了 `[marketplace] auto_update_interval_hours` 的宿主会在扫掠第一个 tick 把 324 MiB 拉回来；
   占位行的幂等是**非破坏**的：同 id 已存在（已下载 / 已移除）一律原样返回，重启不清目录、也不复活
   用户删掉的市场。
2. **`agent-store init` 模板改为注释示例**（`apps/agent-store/src/init.rs`）：模板原先把三源写成**活行**，
   而活行正是「启动时下载」的声明——不改模板等于全新安装仍会下载（`30` §7 / §9 以 `init.rs:139`
   写活行为前提，已在 §11 订正）。测试同步改名并加断言：经宿主自己的解析器读到 `default_marketplaces`
   **为空**。
3. **前端**（`web/src/components/catalog/MarketSourcesPanel.tsx` + `web/src/i18n/{zh-CN,en-US}.ts`）：
   以 `resolved_revision` 缺失判定「未下载」（所有取包路径都会记 revision，缺它 = 一个字节都没拉，
   而非「市场为空」）；卡片副标题与「条目数」显示**未下载**，详情页主按钮变**下载**（复用同一
   `market/refresh`，**无新 wire 方法**），toast 文案随「下载 / 检查更新」分流；目录页空态文案改为
   指向「市场源」。**指纹不动、站点仓不动**（`MarketplaceSummary.resolved_revision` 本就是
   `Option<String>`，`to_summary` 直接透传）。
4. **读数**：`cargo test -p nomifun-app-server --lib only_declared_default_marketplaces_are_fetched_at_boot`
   ok；`-p nomifun-app --lib unfetched_registration_stores_a_source_without_downloading_it` ok（真实
   SQLite 仓储，覆盖幂等 / 非破坏 / 同 source 异 id / 软删不复活）；`cargo test -p agent-store`
   **9 passed**；`cd web && bun run typecheck` 0 错误、`bun run test` **513 passed / 1 skipped**；
   `cargo check -p nomifun-app-server --tests`、`-p nomifun-app --tests -p agent-store` 均 exit 0。
   **未做**：`web/` 那个面板至今没有渲染测试（它读 zustand client 并在 mount 时取数，现有多是
   prop 驱动的静态渲染），因此「未下载 → 下载按钮」这一映射没有自动化用例，只有 `tsc` + 人工。

