# 协议指纹与跨仓同步全量手册 (Protocol Fingerprint & Release Sync Guide)

> **定位**：Flowy App Server Wire 协议演进、指纹升级（Fingerprint）、跨仓同步与发版全流程指南。  
> **核心原则**：**“动 App Server wire 协议面 = 换指纹 + 两仓一起改；机械门禁全绿，历史底稿不丢。”**

---

## 1. 为什么需要协议指纹与严格相等校验

Flowy Agent Store 的 App Server 通过 WebSocket / JSON-RPC 暴露公开服务，外部 TS/Python SDK、CLI 及 WebUI 均为协议客户端。

在握手阶段，客户端与服务端会对协议版本号执行**严格相等（Strict Equality）校验**：
- 服务端权威常量：`PROTOCOL_VERSION`（位于 `crates/backend/nomifun-app-server/src/lib.rs`）；
- 客户端权威常量：`APP_SERVER_PROTOCOL_VERSION`（位于 `web/packages/protocol/src/protocol.ts`）。

如果客户端使用的指纹与服务端不一致，连接会在握手期被立即拒绝。因此，任何对 Wire 协议面的变更都必须同步推进全量落点，绝不能留有旧值死角。

---

## 2. 指纹形态规范：`fp-<n>` 单调计数器

### 2.1 为什么废弃日期戳形式
在早期版本中，协议指纹曾采用形如 `2026-09-21` 的日期戳。实践表明这会带来严重误解：
1. **日历超前与困惑**：当一天内发生多次连续变更时，为了保证指纹单调递增，开发者常常手动将日期加 1 天（导致版本号日期比实际日历超前数天）；
2. **误读为发布日期**：下游使用者和开发者极易将协议指纹误认为是软件发版日期；
3. **正则扫描误伤**：全仓扫描 `"20xx-xx-xx"` 时，会与 MCP 协议版本（如 `2025-11-25`）、历史夹具 `published_at`（如 `2000-01-01`）等无辜字符串混淆。

### 2.2 现行形态规范
- 当前统一采用纯计数器形态：**`fp-<n>`**（例如 `fp-1`, `fp-2`, ..., `fp-13`）；
- 计数器保留了指纹唯一有用的性质——**全序与单调性**；
- 每次协议面变更后，计数器**严格加 1**，绝对禁止复用或回滚历史指纹值。

---

## 3. 触发指纹升级的范围 (Wire 变更触发条件)

只要改动触及 App Server 的公开通信界面，哪怕是**增量改动**，也必须递增指纹：
1. **API 方法变更**：新增、删除或重命名任何协议方法（如新增 `market/settings`）；
2. **DTO 结构变更**：在既有请求体（Request）、响应体（Response）或设置结构中增加、修改或删除字段；
3. **事件 Payload 变更**：实时通知事件（Notification）、消息流（Delta）、转写事件中的字段或枚举增减；
4. **服务端主动请求（Server Request）**：新增审批类型或回调参数定义。

---

## 4. 全量落点矩阵与门禁实现原理

### 4.1 全量落点清单 (7 个本仓文件 10 处 + 独立站点仓 2 处)

每当升级指纹（例如 `fp-12` → `fp-13`），必须同步修改以下全部落点：

| 仓库 | 文件路径 | 对应常量或断言内容 |
|---|---|---|
| **本仓 (allo)** | `crates/backend/nomifun-app-server/src/lib.rs` | `pub const PROTOCOL_VERSION: &str = "fp-13";` (真源) |
| **本仓 (allo)** | `web/packages/protocol/src/protocol.ts` | `export const APP_SERVER_PROTOCOL_VERSION = "fp-13";` (真源) |
| **本仓 (allo)** | `web/packages/client/src/http-transport.ts` | 请求头或握手载荷中的指纹字段 |
| **本仓 (allo)** | `web/packages/sdk/src/readiness.test.ts` | 单元测试中的指纹期望值断言 |
| **本仓 (allo)** | `web/scripts/mock-server.ts` | Mock 服务端握手响应体中的协议版本 |
| **本仓 (allo)** | `web/scripts/smoke.ts` (两处) | 冒烟测试中的客户端构造参数与期望断言 |
| **本仓 (allo)** | `scripts/probe-agent-store-runtime.mjs` | 外部探测诊断脚本中的默认协议版本 |
| **站点仓 (agent-store-site)** | `content/docs/zh-CN/typescript-sdk.md` | 中文 SDK 使用文档中的常量示例 |
| **站点仓 (agent-store-site)** | `content/docs/en-US/typescript-sdk.md` | 英文 SDK 使用文档中的常量示例 |

### 4.2 机械门禁脚本原理解析

为了防止人工维护遗漏，仓库内置了两套机械门禁：

1. **`bun run check:fingerprint` (`scripts/check-protocol-fingerprint.mjs`)**：
   - **基于标识符精确匹配**：不按通用形状匹配，而是根据 AST / 变量标识符提取（如 `APP_SERVER_PROTOCOL_VERSION = "..."`）；
   - **模式失配即失败（Fail-Closed）**：如果某个落点的代码行被重构导致正则没有命中，门禁会**显式报错退出**，坚决不静默放行；
   - **跨仓可选检查**：当环境变量 `AGENT_STORE_SITE_DIR` 存在或同级存在 `agent-store-site` 检出时，自动连带检查站点仓；不存在时打印提示跳过。
2. **`bun run check:release-sync` (`scripts/check-agent-store-release-sync.mjs`)**：
   - **方法路由数量锁步**：对比 `web/packages/client/src/http-transport.test.ts` 中的 `DOCUMENTED_ROUTE_SPLIT` 与文档记录；
   - **版本号锁步**：确保 `web/packages/protocol/package.json` 与站点仓 `content/release.json` 的版本号严格一致。

---

## 5. 跨仓同步与发版标准 5 步走

当发生 Wire 变更时，必须严格执行以下流水线：

```text
第 1 步: 递增常量并全仓扫描
         修改 nomifun-app-server 与 protocol.ts 中的真源常量 (fp-n -> fp-(n+1))
         使用旧指纹在全仓执行 grep，收敛 mock-server、smoke.ts 等夹具中的残留

第 2 步: 执行机械门禁自查
         运行 bun run check:fingerprint，确保本仓 10 处落点严格一致全绿

第 3 步: 同步基线技术文档
         更新 docs/agent-store/05-flowy-agent-store-app-server-protocol.md 头部版本与改动章节
         更新 docs/agent-store/README.md 历次发布与状态记录

第 4 步: 跨仓同步独立站点 (agent-store-site)
         检出同级 C:\workspace\agent-store-site
         更新 content/docs/{zh-CN,en-US}/typescript-sdk.md 的指纹常量示例
         更新 changelog 与方法计数，确保 bun run check:release-sync 通过

第 5 步: 全量测试验证与发版准出
         本仓执行: cargo test -p nomifun-app-server
         Web 包执行: cd web && bun run typecheck && bun run test
         冒烟测试: bun scripts/smoke.ts
         若需发版，严格遵循 docs/agent-store/25-release-runbook.zh.md 执行 npm 发包与站点上线
```
