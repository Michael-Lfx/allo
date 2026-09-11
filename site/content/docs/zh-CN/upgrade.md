# 升级与迁移指引

本页回答三个问题：**能不能升**、**升到哪个版本**、**怎么升**。适用范围：`@flowy-agent-store/{protocol,client,sdk}` 三个 npm 包，以及随它们一起分发的平台运行时包。

发布事实与升级步骤都在这里；协议方法语义全集仍在仓库文件 `docs/agent-store/05-allo-app-server-protocol.md`。

## 1. 兼容性承诺（当前口径）

**beta 期间不承诺向后兼容。** 三个包都是 `0.1.0-beta.*` 预发布，API 未冻结：类型收窄、方法增删、事件面调整都可能在 beta 线内部发生。

配套两条约定（口径 D10=A 已拍板）：

- 破坏性变更**走 minor 号**（`0.1.x` → `0.2.0`）——现在还没有稳定版可破坏，所以不做 major 号；
- 每次发布的变更类型在 **[变更日志](/zh-CN/docs/changelog)** 明示（见 §9）。

由此得到两条操作建议：

- 生产接入请**固定确切版本**（见 §5），不要依赖裸 `bun add` / `npm install` 的解析结果（见 §4）；
- 升级前先读目标版本的 [changelog 条目](/zh-CN/docs/changelog)，再按 §6 的逐版本步骤操作。

## 2. 已发布的版本序列（事实）

下表由注册表元数据与已发布产物核对得出（复现命令见 §8）：

| 版本 | 发布时间（UTC） | 当前 dist-tag | 与上一版的实质差异 |
| --- | --- | --- | --- |
| `0.1.0` | 2026-09-09T09:09:04Z | 无 | 首次发布；三个包的**代码与类型声明与 beta.2 逐字节相同**，只有 `package.json` 不同：当时的 sdk 没有 `optionalDependencies`（未随附平台运行时包） |
| `0.1.0-beta.2` | 2026-09-09T09:27:36Z | `latest` | 补齐 sdk 的 `optionalDependencies`（darwin / linux / win32 五个平台运行时包，同一版本号） |
| `0.1.0-beta.3` | 2026-09-10T04:44:34Z | `beta` | 为 client / sdk 新增公开声明（重连生命周期、退出观测，见 §6.1）；`package.json` 补 `engines.node >= 22`、`repository`、`sideEffects`；protocol 声明逐字节未变 |

注意 `0.1.0` 是**时间最早**的一次发布（比 beta.2 早约 18 分钟），却没有任何 dist-tag 指向它；它既不是稳定版，也不比 beta 线新。

## 3. dist-tag 语义

`dist-tag` 是发布者可以移动的标签，与版本号大小无关：

| dist-tag | 当前指向 |
| --- | --- |
| `latest` | `0.1.0-beta.2` |
| `beta` | `0.1.0-beta.3` |

`0.1.0` 不带任何 tag。三个包与 `@flowy-agent-store/runtime-*` 两个 tag 的指向一致（已实测）。

```bash
npm view @flowy-agent-store/sdk versions dist-tags --json
```

```json
{
  "versions": ["0.1.0-beta.2", "0.1.0-beta.3", "0.1.0"],
  "dist-tags": { "beta": "0.1.0-beta.3", "latest": "0.1.0-beta.2" }
}
```

两个陷阱：

1. `latest` **不是**最新版——它指向 `0.1.0-beta.2`；最新的 beta 是 `beta` tag 指向的 `0.1.0-beta.3`。
2. `0.1.0` 虽然没有 tag，但版本范围仍可能解析到它（见 §4）。

另外，`versions` 数组的**排列顺序不是时间顺序**：`0.1.0` 排在最后，却是最早发布的。

## 4. 裸安装与范围解析

裸安装走 `latest`，所以拿到的是 `0.1.0-beta.2`；更麻烦的是 bun 会把**范围**写进 `package.json`（`^0.1.0-beta.2`），下一次安装会重新解析——而同一个范围在不同工具里的解析结果并不相同。

实测（临时目录、空 `node_modules`）：

```bash
bun add @flowy-agent-store/sdk                     # → 0.1.0-beta.2（latest）
bun add @flowy-agent-store/sdk@beta                # → 0.1.0-beta.3
bun add '@flowy-agent-store/sdk@^0.1.0-beta.2'     # → bun 解析到 0.1.0-beta.2
npm view '@flowy-agent-store/sdk@^0.1.0-beta.2' version   # → npm 解析到 0.1.0-beta.3
npm view '@flowy-agent-store/sdk@>=0.0.0' version          # → 0.1.0（那个无 tag 的早期发布）
```

结论：**不要依赖范围解析**。用 tag 别名也不安全——`latest` 会在下次发布时被移动（`beta` 同样），今天写 `@beta` 明天可能装到别的版本。请把确切版本写进 `package.json`。

## 5. 固定确切版本（推荐做法）

```bash
# 明确写出确切版本；不要用 ^ 或 ~
bun add @flowy-agent-store/sdk@0.1.0-beta.3
bun add @flowy-agent-store/protocol@0.1.0-beta.3   # 需要协议类型时

# npm / pnpm 同理
npm install @flowy-agent-store/sdk@0.1.0-beta.3
```

`package.json` 里应当落到 `"@flowy-agent-store/sdk": "0.1.0-beta.3"`（**不带** `^`）。sdk 的平台运行时包由它的 `optionalDependencies` 按同一版本号钉住，不需要单独声明。

锁文件也要提交进版本库：`bun.lock` / `package-lock.json` / `pnpm-lock.yaml` 是「这次到底装了什么」的唯一权威记录。

于是升级就是：改这一个字符串、重装、重跑测试，没有别的步骤。

## 6. 逐版本升级步骤

### 6.1 从 0.1.0-beta.2 升到 0.1.0-beta.3

这是目前唯一一次真实的 beta 内部跳跃，**无需改代码**：实测差异只有新增声明（client 的 `TransportLifecycle` / `onLifecycle` / `connectTimeoutMs` / 订阅 `rearm()`；sdk 的 `assertProtocolCompatible`、`SpawnOptions.env` / `cwd` / `onExit`、`SpawnedServer.exited`、`SpawnExitInfo`）与 `package.json` 元数据；protocol 的类型声明逐字节未变。

```bash
# 1) 先看当前实际装的是什么
bun pm ls | grep '@flowy-agent-store'          # npm 项目：npm ls @flowy-agent-store/sdk
# 2) 固定到目标版本（需要几个包就升几个，版本号保持一致）
bun add @flowy-agent-store/sdk@0.1.0-beta.3
bun add @flowy-agent-store/protocol@0.1.0-beta.3
# 3) 确认解析结果
node -p "require('@flowy-agent-store/sdk/package.json').version"
# 4) 重跑自己的类型检查与测试
bun run typecheck && bun run test
```

如果你用 `AGENT_STORE_BIN` 指向自建二进制，请注意 SDK 会**校验就绪行里的 `protocol_version` 与自己是否一致**（见 TypeScript SDK 使用指南 §2）：升级 SDK 之后旧二进制会被拒绝，请在同一步里一并更新二进制。

### 6.2 从 0.1.0 回到 beta 线

`0.1.0` 是首次发布且不带任何 dist-tag；三个包的**代码与类型声明与 `0.1.0-beta.2` 逐字节相同**，差别只在 `package.json`——尤其当时的 sdk **没有** `optionalDependencies`，不会随附 `@flowy-agent-store/runtime-win32-x64` 等平台运行时包，`launchClient` 可能因此找不到可执行文件。

```bash
# 从无 tag 的 0.1.0 切到当前 beta 线
bun add @flowy-agent-store/sdk@0.1.0-beta.3
bun pm ls | grep '@flowy-agent-store'   # 确认 0.1.0 已经不在了
```

这次迁移同样是纯加法：`0.1.0` 到 `beta.3` 的公开声明面只增不减（`beta.2` 补的运行时包、`beta.3` 补的重连与退出观测），没有删除或改名。

## 7. 自查当前安装的版本

```bash
# 项目实际解析到的版本（先看这条）
bun pm ls | grep '@flowy-agent-store'      # npm：npm ls @flowy-agent-store/sdk
# 直接读已安装包的 package.json（三个包都导出 ./package.json）
node -p "require('@flowy-agent-store/protocol/package.json').version"
# 锁文件钉住的版本
grep -o '@flowy-agent-store/sdk@[0-9][^"]*' bun.lock | head -1
# 注册表上有什么、tag 指向哪
npm view @flowy-agent-store/sdk versions dist-tags --json
```

三者不一致时，以**锁文件与已安装包的 `package.json`** 为准：`package.json` 里的范围只表达意图，装进 `node_modules` 的才是事实。

## 8. 未发布的差异（工作区）与自查方法

工作区（`web/packages/*`）的版本号同样是 `0.1.0-beta.3`，但已包含**尚未发布的破坏性改动**：`event_type` 从「开放联合 + `| string` 兜底」收窄为封闭类型 `ConversationEventType`，并新增包内解码器 `decodeConversationEvent`。

已核实的边界：**三个已发布版本都没有这个收窄**——从注册表取产物检查声明即可复现：

```bash
npm pack @flowy-agent-store/protocol@0.1.0-beta.3 --silent
tar xzf flowy-agent-store-protocol-0.1.0-beta.3.tgz
grep -n 'event_type' package/dist/index.d.mts
# 0.1.0 / 0.1.0-beta.2 / 0.1.0-beta.3 三者的 index.d.mts 都是 20049 字节，逐字节相同
# event_type: "message.created" | ... | "context.usage" | string;   ← 那个 | string 还在
```

含义：把 `event_type` 当 `string` 使用的代码（自己拼字符串比较，或靠 `switch` 的 default 兜住未知类型）今天能装能跑；收窄一旦发布，穷尽 `switch` 与类型守卫会开始报类型错误，你需要按封闭联合处理。**它不会静默发生**——这属于破坏性变更，按 §1 的口径随下一次发布（minor 号）在 changelog 明示。本页不预告发布日期。

## 9. changelog 与 release notes 的边界

| 事项 | 现状 | 依据 |
| --- | --- | --- |
| 破坏性变更的版本号 | 走 minor 号，并在 changelog 明示 | 兼容性口径 D10=A（beta 期不承诺向后兼容） |
| 独立 changelog / release notes 页 | ✅ **已建设**：[变更日志](/zh-CN/docs/changelog) | 计划文档 `16` 的 R6（批 4 落地） |
| 本页职责 | 只登记已发布的版本事实与已核实的未发布差异 | 不发明版本历史 |
| 本页与 changelog 的分工 | 本页讲**怎么升**；changelog 讲**每一版改了什么**，并且是破坏性变更的唯一公告面 | 两页交叉引用，不互相复制 |

因此本页给出的是**已发生的事**（发布时间、dist-tag、产物差异）与**工作区已确认但未发布的事**（§8）。任何未在此列出的变更，不要假定它已经发生、也不要假定它不会发生。

## 10. 另见

- [TypeScript SDK 使用指南](/zh-CN/docs/typescript-sdk)：安装、API、事件、错误模型与示例。
- [快速开始](/zh-CN/docs/quick-start)：终端用户的安装包路径。
- [兼容性矩阵](/zh-CN/docs/compatibility)：平台与来源格式的支持范围。
- [变更日志](/zh-CN/docs/changelog)：每个已发布版本改了什么，以及破坏性变更的公告面。
