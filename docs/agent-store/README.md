# Agent Store 文档索引

> 最后核对：2026-09-11（新增 `22-webui-productionization.zh.md`；`16` C 档二次复核改判；**版本框架订正：发版前只有一个版本，统一称 v1，见 `16` §7 决策 4**；引用/口径统一 + 证据与 `TC-*` 正文并入编号文档）
> 用途：本目录文档的地图、权威顺序与状态图例。新会话/新成员先读本文，再按需深入。
> 维护约定：新增文档编号顺延（当前到 `22`）；状态变更时同步更新本索引与文档头部“更新”行；公共契约变更先写 `16-sdk-webui-site-priority-plan.zh.md` §7（决策记录），再改基线文档。

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
| `04-allo-runtime-adapter.md` | Runtime Adapter（Preset → ExecutionParticipant） | 🧊 基线（单 Agent 已实证） |
| `05-allo-app-server-protocol.md` | **App Server 协议 v1**（发版前只有一个版本） | ✅ 现行正文（可改）+ 单 Agent 实测 |
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
| `21-open-decisions.zh.md` | **开放决策书**：D1–D13，逐条带 ⭐ 默认建议与解锁范围（`16` §5.2 剩余任务总表的拍板入口） | ✅ 已拍板（2026-09-10；D13 卡点四档已批准，含 2026-09-11 C 档二次复核改判） |
| `22-webui-productionization.zh.md` | **WebUI 生产化立项**（方向四 + WP-5 协议词汇与概念对齐，来源 `16` R33）：11 项拆为**安全类 / 可观测类 / 功能类 / 协议词汇对齐**，逐条给「可验收条目 + 边界 + 依赖」；含**不做假保护**红线与 V1–V4 未决 | 📋 已立项（2026-09-11），未排期 |
| `19-webui-codex-alignment.zh.md` | **WebUI 子计划**：Codex app 体验对齐基线 + 四层工作包 W1–W14 | 🔧 部分落地（W1 / W5 / W8 / W13 已交付；其余见 R 表） |
| `15-store-chain-and-protocol-vnext-plan.zh.md` | 四链路闭环 + 协议词汇与概念对齐（WP-1~WP-7；原称「协议 vNext」） | 🔧 WP-1/2/3/4/6 完成，WP-7 模型选择器完成；WP-5 顺延至 `16` |
| `13-p0-execution-plan.md` | P0-A/B/C/D 执行细则与出口条件 | 🔧 P0-A/B 已关闭；P0-C/D 的 OAuth 运行时证据已完成（`06-connector-oauth-security.md` §12，26/26 PASS） |
| `16-sdk-webui-site-priority-plan.zh.md` §6 / §7 | 阶段基线与决策记录（原 `agent-store-v1-roadmap.md` §1–§6 / §9 / §10） | 🧊 基线 |
| `agent-store-v1-test-cases.md` | **TC 总索引 + 公共口径**（§1/§2；`TC-*` 逐条正文归各归属文档，见其 §3） | 🧊 基线 |
| `12-sdk-packaging.md` | SDK 封装与发行（P0/P1 已落地，P2 待做） | 🔧 进行中 |
| `14-binary-size-trimming.zh.md` | 二进制瘦身（feature gate 已落地） | 🔧 待 release 重构建验证 |
| `09-release-readiness.md` | 发布门禁与准入判断 | 📋 blocked（快照见 §9） |
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
| `20-tool-injection-policy.zh.md` | **Store 会话工具注入取舍**：保留/关闭清单、两类延迟、关闭机制与坑、`nomi_delegate` 特例 | 📋 设计规格（未实施） |

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
