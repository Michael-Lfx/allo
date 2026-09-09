# Agent Store 文档索引

> 最后核对：2026-09-09
> 用途：本目录文档的地图、权威顺序与状态图例。新会话/新成员先读本文，再按需深入。
> 维护约定：新增文档编号顺延（当前到 `15`）；状态变更时同步更新本索引与文档头部“更新”行；公共契约变更先写 `agent-store-v1-roadmap.md` §10 决策记录，再改基线文档。

## 状态图例

| 标记 | 含义 |
|---|---|
| 🧊 冻结基线 | 架构/公共契约基线，改动需走决策记录（`roadmap §10`） |
| ✅ 已实现 | 代码落地并有实证（指向证据页） |
| 🔧 部分实现 | 主体落地，尾项未完成 |
| 📋 计划 | 已排期/已定义，未开工或进行中 |
| 📎 证据 | 运行时实测快照（有时间戳，非契约） |
| 🗄️ 历史 | 被后续文档取代，保留追溯 |

## 一、权威基线（先读这组）

| 文档 | 内容 | 状态 |
|---|---|---|
| `00-architecture-decision.md` | 架构边界、组件职责与取舍 | 🧊 冻结（部分已实证） |
| `01-domain-model.md` | 领域模型（Agent/Team/Skill/Connector、Execution） | 🧊 冻结 |
| `02-codebuddy-workbuddy-import-spec.md` | Importer 规格（CodeBuddy/WorkBuddy 导入） | ✅ 已实现 |
| `03-codebuddy-compatibility-matrix.md` | 三态兼容矩阵与推导规则 | ✅ 规则已实现 |
| `04-allo-runtime-adapter.md` | Runtime Adapter（Preset → ExecutionParticipant） | 🧊 冻结（单 Agent 已实证） |
| `05-allo-app-server-protocol.md` | **App Server 协议 v1 基线** | 🧊 冻结 + ✅ 单 Agent 实测 |
| `06-connector-oauth-security.md` | Connector OAuth 与安全模型 | 🧊 冻结（OAuth 已实证） |
| `07-typescript-sdk.md` | **TS SDK v1 基线**（包名以 `12` 为准） | 🧊 冻结（主体已实现） |
| `08-flowy-web-integration.md` | Flowy/Web 集成面 | 🧊 冻结 |
| `10-public-contracts.md` | 公共契约（状态、事件、错误、幂等、认证） | 🧊 冻结 |
| `11-webui-production-readiness.md` | WebUI 生产就绪差距清单 | 📋 规划中 |

## 二、计划与执行（按权威顺序）

| 文档 | 内容 | 状态 |
|---|---|---|
| `15-store-chain-and-protocol-vnext-plan.zh.md` | **当前执行顺序**：四链路闭环 + 协议 vNext（WP-1~WP-7） | 📋 计划冻结 2026-09-09 |
| `13-p0-execution-plan.md` | P0-A/B/C/D 执行细则与出口条件 | 📋 起草（Step 1 待开工） |
| `agent-store-v1-roadmap.md` | 阶段基线与决策记录（§10） | 🧊 冻结 |
| `agent-store-v1-test-cases.md` | 测试主表（`TC-*` 用例唯一正文） | 🧊 冻结 |
| `12-sdk-packaging.md` | SDK 封装与发行（P0/P1 已落地，P2 待做） | 🔧 进行中 |
| `14-binary-size-trimming.zh.md` | 二进制瘦身（feature gate 已落地） | 🔧 待 release 重构建验证 |
| `09-release-readiness.md` | 发布门禁与准入判断 | 📋 blocked（快照见 §9） |
| `开发计划.md` | 早期开发总纲 | 🗄️ 历史（见其头部指针） |
| `技术方案.md` | 早期技术总纲 | 🗄️ 历史（见其头部指针） |

## 三、证据（运行时实测快照）

| 文档 | 覆盖 |
|---|---|
| `importer-runtime-evidence.zh.md` | TC-IMP-001~009（Importer） |
| `mcp-oauth-runtime-evidence.zh.md` | OAuth 登录 → 注入 → 401 刷新（真实 MCP 服务） |
| `single-run-runtime-evidence.zh.md` | TC-RT-001 单 Agent Run（mimo-v2.5 真实模型） |

## 四、设计记录

| 文档 | 内容 | 状态 |
|---|---|---|
| `nomifun-mcp-oauth-dynamic-client-registration-design.md` | MCP OAuth 动态客户端注册设计 | ✅ 已实现 |

## 阅读顺序建议

- **接手开发**：本索引 → `15`（当前顺序）→ `05`/`07`（协议与 SDK 基线）→ `13`（P0 细则）→ 证据页
- **了解架构**：`00` → `01` → `04` → `05` → `10`
- **对接 SDK**：`12` → `07` → `05`
- **发布评估**：`09` → `agent-store-v1-test-cases.md`

## 已知待修（P1，不阻塞主线）

1. **冻结文档状态行**：`00/01/04/06/07/08` 头部状态行多为 2026-08-26 快照，未逐份刷新（当前状态以本索引为准）。
2. **App Server stdio 口径**：架构文档（`00/05/06/07/08/10`）按“本地可信进程（stdio 或 WebSocket）”表述；实现与交付范围以 `roadmap §10` 决策为准——**stdio 不纳入**。P1 统一口径。
3. **证据文件位置**：三份证据页散在根目录，后续可归入 `evidence/` 子目录（需同步改跨文档链接）。
4. **规划文档重叠**：`开发计划.md`/`技术方案.md` 与 `roadmap`/`13`/`15` 内容有重叠，保留为历史追溯，不再更新正文。
