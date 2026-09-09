# Agent Store SDK 封装技术方案（对标 Codex SDK）

> 状态：P0 已实现并实测；P1-1/P1-2/P1-3 主体已落地（tsdown 编译 + npm dry-run 通过）；P1-4 已决策延后；P2 未开始
> 更新：2026-09-04（P0 状态行此前写“P1/P2 未开始”，已过期更正）
> 日期：2026-09-04
> 前置：`00-architecture-decision.md`、`05-allo-app-server-protocol.md`（§2.1.1 传输绑定）、`07-typescript-sdk.md`、`10-public-contracts.md`
> 目标：以 `agent-store` 独立二进制为 runtime，提供可发布的 TS/Python SDK；SDK 与 Web 同走一套协议方法

## 1. 背景与结论

Codex SDK 的形态是“可被拉起的本地 runtime + typed client”：`codex app-server --listen stdio://` 子进程 + JSON-RPC。allo 具备对应物的雏形：

- `apps/agent-store` 已是独立二进制（后端 + 内嵌 Web UI，同端口，默认 `127.0.0.1:8787`，回环本地可信免登，`--auth` 切认证模式）；
- 协议层已统一：`dispatch_connection_request` 为唯一分发，`import/install/market/store/workspace` 等方法 WS/HTTP 同名同实现（`05 §2.1.1`）；
- 前端已有 `Transport` 抽象（`web/src/lib/transport.ts`）与全量 typed client（`web/src/lib/*`）。

因此最短路径**不需要等 stdio**：SDK 以“spawn 二进制 → 连回环 WS”平替 Codex 的“spawn → 连 stdio”。stdio 可作为后续优化（`main.rs` 已有 MCP stdio 子命令先例，有地方挂）。

## 2. 非目标

- 不实现 stdio 传输（`LocalTransport::Stdio` 保持仅枚举值）；
- 不做远端托管/多租户（`00 AD-08` 仍为非目标）；
- 不收编公开资产 `GET` 与 `/api/fs/browse`（非协议方法，见 `05 §2.1.1`）。

## 3. 总体结构

```text
@flowy-agent-store/protocol   纯类型（请求/响应/通知/错误），零运行时依赖
@flowy-agent-store/client     AppServerClient + 子客户端 + Transport 接口
@flowy-agent-store/sdk          spawn 二进制 + WS 建连 + 通知分发/重连/Approval 钩子
@flowy-agent-store/browser    WS 绑定 + <img> 资产 URL 辅助（Web/Flowy 用）
python-sdk              Popen spawn + reader 线程 + typed 方法（对标 Codex client.py）
```

`07 §2.3` 的包拆分即按此落地；`web/src/lib` 是各包的抽取来源，不是发布物（`web` 包保持 `private`）。

## 4. P0：运行契约（`apps/agent-store`）

| # | 事项 | 说明 |
|---|---|---|
| P0-1 | 就绪协议 | ✅ 已实现：`--port 0` + bind 后 stdout 单行 `{"agent_store":"listening",host,port,url,protocol_version,version,auth}`（SDK 扫描该行；无秘密）。 |
| P0-2 | 版本探测 | ✅ 已实现：`--version`（clap 自带）+ 就绪行同时带 `version` 与 `protocol_version`（`PROTOCOL_VERSION` 已 `pub`）；SDK 仍需按 `initialize` 返回做兼容检查。 |
| P0-3 | 数据目录隔离 | ✅ 已实测：第二实例同 data_dir 被单实例锁干净拒绝（`already in use by another running Flowy backend (pid …)`），无损坏风险。结论：SDK 必须自带临时 `--data-dir`（文档写死独占）。 |
| P0-4 | 回环强制 | ✅ 已落地：`web/src/lib/transport.ts:isLoopbackUrl`（`127/8`、`::1`、`localhost`）+ SDK `launchClient` 非回环拒绝（e2e 覆盖）。 |

## 5. P1：包拆分

| # | 事项 | 来源与改动 |
|---|---|---|
| P1-1 | `@flowy-agent-store/protocol` | ✅ 已落地：`web/packages/protocol`（`protocol.ts`+`errors.ts`，`git mv` 保留历史），`web` 经 `workspaces` + 精确版本引用；旧路径为单行 re-export 垫片。tsdown 编译（esm+cjs+dts），裸 Node ESM/CJS 消费验证通过。 |
| P1-2 | `@flowy-agent-store/client` | ✅ 已落地：`web/packages/client`（`Transport`+基类+7 子客户端，`transport` 必填）；Web 3 方法（`serverRootUrl`/`browseDirectory`/`registerWorkspace`+HTTP 底座）由 `web/src/lib/client.ts` 子类承载，调用点零改动。tsdown 编译，`dev/test/typecheck/build` 前置 `build:packages`（热构建约 8s，`dist/` 不入库）。 |
| P1-3 | `@flowy-agent-store/sdk` | 🔧 主体已落地（Approval 钩子除外）：`web/packages/sdk`（spawn+就绪扫描+回环建连+版本检查+退出清理），e2e 实测通过（覆盖修订后的 TC-SDK-001：spawn + 回环 WS）；tsdown 编译；npm publish dry-run 通过。`MessageRouter`/重连复用基类订阅原语，`approval/request` 等解冻。 |
| P1-4 | Python SDK | ⏸️ 已决策延后（2026-09-04）：优先完善已有 TS 功能。结构对标 Codex `client.py:CodexClient`（`Popen` + reader 线程 + pending 表）；类型用 pydantic 与 `protocol` 对齐。 |

## 6. P2：发行与文档

- **二进制分发**（已定案 2026-09-09，走 npm optionalDependencies）：发布一组按平台的 runtime 包（`@flowy-agent-store/runtime-<platform>-<arch>`，如 `runtime-win32-x64`，每个包内置 `agent-store[.exe]`），作为 `@flowy-agent-store/sdk` 的 `optionalDependencies` 加载；`resolveAppServerBin` 查找顺序改为：`bin` 参数 → `AGENT_STORE_BIN` 环境变量 → **`require.resolve` 定位 platform 包内二进制** → PATH。runtime 包与 `protocol/client/sdk` 同版本锁步，客户额外传入 `bin` 时跳过包查找。这解决“SDK 已发布但代码库外拿不到 `agent-store` 可执行文件”的核心缺口。备选（GitHub releases + checksum 下载缓存）不采用。
- **版本政策**：协议版本起 changelog；Team 完整能力、事件 cursor 追平（V2）等未稳能力在 SDK 层标 experimental（参考 Codex `experimental_api`），可暂不暴露。
- **同机声明**：`import/run`、`workspace/create`、`market/* directory` 要求 client 与 server 同文件系统；文档明确，远端场景 SDK 对这类方法前置拒绝。
- **文档三件套**：getting-started / api-reference / examples（对标 Codex SDK 的 `docs/` + `examples/` 布局）。

## 7. 测试与验收

- SDK 级 roundtrip（spawn 真实二进制 + 临时 data_dir）：`initialize → store/list → store/install-entry → agent/run → run/events → run/result` 全绿。
  - 2026-09-04 进展：`agent/run → run/get → run/result → run/events` 子链已绿（TC-RT-001，见 `single-run-runtime-evidence.zh.md`）；
    `store/install-entry` 产品路径未走（A1 用 import/install 直调），待补。
- 与冻结文档 `07-typescript-sdk.md` 的差异：包名以本文为准（`@flowy-agent-store/sdk`；`07` 已于 2026-09-09 同步改名并加注）；
  `Transport` 必填、`exports` 直指 `dist` 等包边界结论同样以本文为准。
- 双开冒烟：桌面端 + SDK 实例同机运行，无锁库、无跨 owner 数据串扰。
- 远端 URL 被 SDK 明确拒绝；协议版本不匹配时报错信息包含两端版本号。
- 全程 SDK 不直调 HTTP（公开资产 `<img>` 除外）；新增协议方法时 TS/Python 类型与 `dispatch` arms 同步更新（`10-public-contracts` 为准）。

## 8. 风险

1. 协议仍在加方法（本次即新增 14 个 WS arms），SDK 发布后 breaking 成本高——靠 P2 版本政策 + experimental 门缓解。
2. 事件语义是尽力而为（可丢、可乱序），远端/跨进程场景下 SDK 侧重连追平依赖 `run/events` cursor（半成品），`node` 包须自带去重排序。
3. `import` 的本地路径语义与远端天然冲突，见 §6 同机声明。
4. 当前 `web/src/lib` 仍有他处并行修改（如 `protocol.ts`），抽包时以 `10-public-contracts` 为准做一次性对齐，避免把进行中的 UI 改动带进 SDK。
