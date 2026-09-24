# `@flowy-agent-store/sdk`

Node.js host for the Agent Store runtime (`docs/agent-store/12-sdk-packaging.md P1-3`): spawn `agent-store`, wait for its stdout readiness line, connect over loopback with `@flowy-agent-store/client`.

```ts
import { launchHarness } from "@flowy-agent-store/sdk";

const harness = await launchHarness({
  client: { name: "my-app", version: "0.1.0" },
});

// 获取：浏览 / 搜索目录（含每个条目的安装状态）
const items = await harness.store.search("software");

// 安装：导入 + 注册，并等到组件真的可用（默认 waitForReady: true）
const outcome = await harness.store.install(items[0]);
if (outcome.readyIssue === "authorization_required") {
  const id = outcome.readyComponentId!;
  // 失败有两条通道，别只看一条：浏览器**之前**的失败由 authStart 同步回
  // state:"error"；**之后**的失败（换 token 被拒、回调超时、被限流）只会出现在
  // authStatus 的 error 里，state 一直停在 not_authenticated。
  // waitForAuth 把第二条通道包成一次调用，并在结束时说清原因。
  const started = await harness.connectors.authStart(id);
  if (started.state === "error") throw new Error(started.error ?? "authorization failed to start");
  const auth = await harness.connectors.waitForAuth(id);
  if (auth.state !== "authenticated") {
    throw new Error(auth.state === "error" ? auth.error : "timed out in the browser");
  }
}

// 禁用 / 启用：连接器改 mcp_servers.enabled，专家/专家团改 Preset.enabled
await harness.store.setEnabled(items[0], false);

// 卸载：真的释放运行时产物（技能目录 / Preset / MCP server 行），再清记录
const released = await harness.store.uninstall(items[0]);
if (!released.ok) console.warn(released.components.filter((c) => !c.ok));

// 一次 close 收掉传输与子进程（退订 → 关传输 → 杀进程 → 删临时 data-dir）
await harness.close();
```

## 导出专家 / 专家团并写到目录（`exportAgent` / `exportTeam` / `materializePack`）

`agents.export` / `teams.export` 只给**定义**（`ExpertPack`），技能是**引用**（`{name, id}`，没有字节）。
下面三个函数把「定义 + 技能字节」一次**写成一个目录**（`docs/agent-store/35`；它把 doc `32` §6.5 的
11 行配方提升成了 API）：

```ts
import { exportTeam } from "@flowy-agent-store/sdk";

// 多 agent 的专家团：递归展开 + 整包失败（成员缺一即失败并点名），leader 在首位
const result = await exportTeam(harness, teamId, "./frontend-backend-experts");
// 单 agent 专家用 exportAgent(harness, agentId, dir)
// 已有 pack（比如缓存/网络传来的）用 materializePack(harness, pack, dir) 只写目录

result.pack;                 // ExpertPack —— 内存里也拿得到，不落盘也能用
result.writtenSkills;        // 实际写入的技能名（去重后）
result.danglingSkills;       // 声明了但本机取不到的技能：{ id, error }，如实上报，不静默跳过
```

写出的布局：

```text
<dir>/
  expert-pack.json           # 线上 pack 逐字节
  persona.md                 # 仅 agent 形态（团没有自己的 persona）
  members/<id>/persona.md    # 仅 team 形态，每个成员一个
  skills/<name>/…            # 成员声明去重后的技能字节
```

错误语义：wire 导出失败（`agent_not_installed` / `agent_disabled` / `policy_denied` /
`version_mismatch` / `response_too_large`…）**先于任何写盘**，不留半成品目录；`skill/files`
失败进 `danglingSkills` 并继续；`skill/file` 在列文件成功后失败则**抛出**——那是宿主 I/O 错误，
吞掉会造出「列了文件却缺内容」的残目录。

**`launchHarness` 解析出的对象就是那个 client**（`docs/agent-store/31` §5 方案 B）：`harness.store` / `harness.conversations` / `harness.models` 直接可用，没有 `.client` 一跳；子进程是 `harness.server`（`readiness` / `dataDir` / `exited`），握手响应是 `harness.handshake`。

五个动词都在 SDK 里可用，但**它们不是 `@flowy-agent-store/sdk` 自己实现的**：本包只负责进程与传输，返回的对象就是一个 `@flowy-agent-store/client` 的 `AppServerClient`（sdk 另挂 `server` / `handshake` / `close()`），其中 `store` 子客户端把 `install` / `uninstall` / `setEnabled` 编排成一条状态机（`search → install → … → uninstall`）。这样分层是刻意的——见 `docs/agent-store/12-sdk-packaging.md` §2 的职责划分。

几条使用上的事实（都是实测结论，不是约定）：

- `store.install` 默认**等到可用**才 resolve。技能拷完即可用，连接器不然：它注册出来是 **disabled** 的（安装器的既定默认），所以就绪检查会先把它 enable，再跑一次探针——只轮询状态永远到不了 `connected`，因为状态派生自上一次探针。探针**有节奏**：首轮（以及之后每 `readyProbeMs`，默认 5s）真连一次，其余轮次只读状态。这是刻意的：一次探针会解析已存 token（临近过期就刷新，401 再刷新一次），按 400ms 的轮询间隔去探针，30 秒预算里能对连接器及其授权服务器打出约 10 次连接 / 最多约 20 次 token 请求，而没有任何人点过按钮。
- 就绪超时**不会丢掉安装结果**：返回成功的安装 + `ready: false` / `readyIssue: "ready_timeout"`。需要 OAuth 的连接器立刻返回 `authorization_required`，不把超时预算烧光。
- `connectors.waitForAuth(id)` 是「等授权完成」的写法：它轮询 `authStatus`，一拿到 `error` 就立刻带原因返回（`{state:"error", error}`），成功返回 `{state:"authenticated"}`，预算用完返回 `{state:"timeout"}`；只有状态读本身失败才 reject。手写 `while (state !== "authenticated")` 循环的典型结局是「它就是没变成 authenticated」——而原因（换 token 被拒、回调超时、`slow_down` 限流）早就躺在它每次读到的 `error` 里。
- 没有「更新」动词。`store.checkUpdates()` 只报告哪些条目有新版，`store.updateHint(item)` 告诉你该怎么做——当前是 `"uninstall_reinstall"`：卸载后重装，安装路径会按版本漂移重新导入，取到市场当前版本。
- 卸载是**可重入**的：产物已不在算成功。部分失败时那些组件保持已安装、`ok: false` 且点名到组件，重试有意义。
- `store.setEnabled(item, false)` 对技能只翻一个目录层标记，返回 `code: "skill_disable_flag_only"`——技能语料没有启用状态，要让技能离开运行时只能 `uninstall`。这个 code 会原样透出，不会被吞。

规则（P0 实测结论）：

- 子进程固定 `--host 127.0.0.1 --no-open`；非回环一律拒绝（`isLoopbackUrl`）。
- 工具面不用改宿主自己的 `~/.agent-store/config.toml`：`env.AGENT_STORE_TOOLS` 直接指定，值是 JSON（形状同 `[tools]` 表），**整份替换**文件里的策略，`{}` = 全开。它只在 Store 宿主生效（桌面 / Web 宿主不采纳 `[tools]`），且子进程启动时读一次。

  ```ts
  const harness = await launchHarness({
    client: { name: "my-app", version: "0.1.0" },
    env: {
      // 只要基础能力：关掉桌面控制/浏览器与几个域
      AGENT_STORE_TOOLS: JSON.stringify({
        web: true,
        computer: false,
        browser: false,
        domains: { cron: false, knowledge: false, media: false },
      }),
    },
  });
  ```
- 默认自带临时 `--data-dir` 并在 `close()` 删除；传自己的目录即表示独占——后端单实例锁会 fail-fast。
- 二进制定位：`bin` 参数 → `AGENT_STORE_BIN` → 平台 runtime 包的 `vendor/` → `PATH`；找不到直接报错（**不下载**，见 P2）。
- 就绪行 `protocol_version` 与 SDK 不一致时杀掉子进程并报错（含两端版本）。该值是**契约指纹**，不是版本号：任何 wire 变更都会 bump。

不在本包范围内（`07-typescript-sdk.md` §非目标）：`config/*` 与 `skill/*` **写**面。它们的**类型**在
`@flowy-agent-store/protocol`（wire 上存在的方法就有形状），但**没有** typed 方法——因为这些面跟着
宿主自己的设置文件与技能目录走，是最易变的部分。**技能的读面是例外**：`skill/files` / `skill/file`
有 typed 方法（`harness.skills.*`），且被上面的导出函数消费。这是可发现性边界，不是权限边界：
`harness.transport` 是公开的，服务端按磁盘归属判定可写性。要用就直连 `transport.request`，并自担变动。
