# 变更日志

本页记录 `@flowy-agent-store/*` 每个**已发布**版本改了什么，并且是**破坏性变更的唯一公告面**。

发布事实（发布时间、dist-tag 指向、产物差异如何核对）在[升级与迁移指引](/zh-CN/docs/upgrade)；本页只回答「每一版改了什么」。

## 1. 本页范围

- **只登记已经发布到 npm 的版本**：`@flowy-agent-store/protocol`、`client`、`sdk`，以及随 sdk 一起分发的 `@flowy-agent-store/runtime-*` 平台包。
- **未发布的改动不构成本页条目**：工作区里已经存在、但尚未随任何版本发布的差异，只登记在[升级与迁移指引](/zh-CN/docs/upgrade) §8 与本文 §4。
- **不预告日期**：本页不出现「即将发布」「计划于」这类表述；条目只在发布**之后**追加。
- 版本号语义与兼容性承诺（beta 期不承诺向后兼容、破坏性变更走 minor 号）见[升级与迁移指引](/zh-CN/docs/upgrade) §1 与本文 §3。

## 2. 已发布版本（事实）

三个包与平台运行时包当前共用同一组版本号。复现命令：

```bash
npm view @flowy-agent-store/sdk versions dist-tags time --json
```

```json
{
  "versions": ["0.1.0-beta.2", "0.1.0-beta.3", "0.1.0"],
  "dist-tags": { "beta": "0.1.0-beta.3", "latest": "0.1.0-beta.2" },
  "time": {
    "0.1.0": "2026-09-09T09:09:04.795Z",
    "0.1.0-beta.2": "2026-09-09T09:27:36.122Z",
    "0.1.0-beta.3": "2026-09-10T04:44:34.609Z"
  }
}
```

| 版本 | 发布（UTC） | 变更类型 | 当前 dist-tag |
| --- | --- | --- | --- |
| `0.1.0-beta.3` | 2026-09-10 | 加法（无破坏性） | `beta` |
| `0.1.0-beta.2` | 2026-09-09 | 加法（无破坏性） | `latest` |
| `0.1.0` | 2026-09-09 | 首次发布 | 无 |

`versions` 的排列顺序**不是**时间顺序：`0.1.0` 排在最后，却是**最早**的一次发布（比 `beta.2` 早约 18 分钟），且不带任何 dist-tag。它不是稳定版，也不比 beta 线新——dist-tag 语义见[升级与迁移指引](/zh-CN/docs/upgrade) §3。

### 2.1 `0.1.0-beta.3` — 2026-09-10T04:44:34Z

- **client**：新增公开声明 `TransportLifecycle`、`onLifecycle`、`connectTimeoutMs`，以及订阅状态机的 `rearm()`。
- **sdk**：新增公开声明 `assertProtocolCompatible`、`SpawnOptions.env` / `cwd` / `onExit`、`SpawnedServer.exited`、`SpawnExitInfo`。
- **`package.json` 元数据**：client 与 sdk 补上 `engines.node >= 22`、`repository`、`sideEffects`。
- **protocol**：类型声明**逐字节未变**。
- **升级影响**：从 `0.1.0-beta.2` 升级**无需改代码**，步骤见[升级与迁移指引](/zh-CN/docs/upgrade) §6.1。

### 2.2 `0.1.0-beta.2` — 2026-09-09T09:27:36Z

- **sdk**：补齐 `optionalDependencies`，按同一版本号钉住五个平台运行时包——`runtime-win32-x64`、`runtime-linux-x64`、`runtime-linux-arm64`、`runtime-darwin-x64`、`runtime-darwin-arm64`。
- 此前的 `0.1.0` 没有这份依赖，`launchClient` 可能因此找不到可执行文件（见[升级与迁移指引](/zh-CN/docs/upgrade) §6.2）。
- 它是 `latest` 标签当前指向的版本——**不是最新版**。

### 2.3 `0.1.0` — 2026-09-09T09:09:04Z

- **首次发布**：三个包的第一个公开版本；没有更早的版本可破坏，因此不含破坏性变更。
- 三个包的**代码与类型声明与 `0.1.0-beta.2` 逐字节相同**，差别只在 `package.json`——sdk 当时**没有** `optionalDependencies`。
- 不带任何 dist-tag。

> 上面「与上一版的实质差异」来自 `npm pack` 取回已发布产物后的逐字节比对，原始记录在计划文档 `16` 的 R4 行与 `21` 的 R4 落地记录；本页只做汇总，不新增判据。

## 3. 变更类型与破坏性变更的公告规则（D10=A）

口径：**beta 期间不承诺向后兼容**，破坏性变更**走 minor 号**（`0.1.x` → `0.2.0`；现在还没有稳定版可破坏，因此不做 major 号）。

| 变更类型 | 版本号 | 本页动作 |
| --- | --- | --- |
| 首次发布 | 该版本号本身 | 新增条目，标「首次发布」 |
| 加法（新增声明、新增依赖、元数据） | patch / 预发布序号 | 新增条目，标「加法（无破坏性）」 |
| 破坏性（类型收窄、方法增删、事件面调整） | **minor 号** | 新增条目，标「破坏性」并给出迁移做法 |

**本页是破坏性变更的唯一公告面**：它不靠 commit message、聊天记录或 release 页通知——只有本页条目写了「破坏性」并给出迁移做法，才算公告（版本号规则见[升级与迁移指引](/zh-CN/docs/upgrade) §1）。

条目的新增与更正规则：

- **新增**：向 npm 发布新版本**之后**追加，不写「预告」条目。
- **永不改写**：已发布条目的版本号与发布时间一经写下不再修改。
- **更正**：条目写错时在同一处追加一行「更正（日期）」说明，不静默改历史。
- **dist-tag 移动不产生条目**：`latest` / `beta` 指向哪个版本属于发布状态，记在[升级与迁移指引](/zh-CN/docs/upgrade) §3。

## 4. 未发布的变更与发布节奏

本页只覆盖已发布的版本。工作区（`web/packages/*`）里已经有、但**尚未随任何版本发布**的改动：

| 事项 | 状态 | 依据 |
| --- | --- | --- |
| `event_type` 从开放联合（带 `\| string` 兜底）收窄为封闭类型 `ConversationEventType`，并新增包内解码器 | 工作区已有，**未发布**：三个已发布版本的声明里 `\| string` 仍在 | [升级与迁移指引](/zh-CN/docs/upgrade) §8；`16` R1 行 |
| 协议方法面增量（`run/plan`、`config/get`、`config/set`、`run/answer-decision`、市场条目快照等） | 工作区已有，**未发布**：已发布的 protocol 产物里没有这些类型 | `16` R2 / R8 / R10 / R16 行 |
| release 重建与 `beta.4` | **挂起**（口径：发布一致性收口延后）；本页不预告日期 | `16` R6 行 |

因此：**没有列在本页的变更，不要假定它已经发布；列在 §4 的，不要假定它已经发布。**

## 5. 另见

- [升级与迁移指引](/zh-CN/docs/upgrade)：发布事实表、dist-tag 语义、固定确切版本、逐版本升级步骤与自查命令。
- [TypeScript SDK 使用指南](/zh-CN/docs/typescript-sdk)：安装、API、事件与错误模型；其 §1 给出当前版本状态。
- [快速开始](/zh-CN/docs/quick-start)：终端用户的安装包路径。
