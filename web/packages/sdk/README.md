# `@flowy-agent-store/sdk`

Node.js host for the Agent Store runtime (`docs/agent-store/12-sdk-packaging.md P1-3`): spawn `agent-store`, wait for its stdout readiness line, connect over loopback with `@flowy-agent-store/client`.

```ts
import { launchClient } from "@flowy-agent-store/sdk";

const session = await launchClient({
  client: { name: "my-app", version: "0.1.0" },
});

// 获取：浏览 / 搜索目录（含每个条目的安装状态）
const items = await session.client.store.search("software");

// 安装：导入 + 注册，并等到组件真的可用（默认 waitForReady: true）
const outcome = await session.client.store.install(items[0]);
if (outcome.readyIssue === "authorization_required") {
  await session.client.connectors.authStart(outcome.readyComponentId!);
}

// 禁用 / 启用：连接器改 mcp_servers.enabled，专家/专家团改 Preset.enabled
await session.client.store.setEnabled(items[0], false);

// 卸载：真的释放运行时产物（技能目录 / Preset / MCP server 行），再清记录
const released = await session.client.store.uninstall(items[0]);
if (!released.ok) console.warn(released.components.filter((c) => !c.ok));

await session.close();
```

五个动词都在 SDK 里可用，但**都不在 `@flowy-agent-store/sdk` 这个包里**：本包只负责进程与传输，方法来自 `@flowy-agent-store/client`，其中 `store` 子客户端把 `install` / `uninstall` / `setEnabled` 编排成一条状态机（`search → install → … → uninstall`）。这样分层是刻意的——见 `docs/agent-store/12-sdk-packaging.md` §2 的职责划分。

几条使用上的事实（都是实测结论，不是约定）：

- `store.install` 默认**等到可用**才 resolve。技能拷完即可用，连接器不然：它注册出来是 **disabled** 的（安装器的既定默认），所以就绪检查会先把它 enable，再跑一次探针——只轮询状态永远到不了 `connected`，因为状态派生自上一次探针。
- 就绪超时**不会丢掉安装结果**：返回成功的安装 + `ready: false` / `readyIssue: "ready_timeout"`。需要 OAuth 的连接器立刻返回 `authorization_required`，不把超时预算烧光。
- 没有「更新」动词。`store.checkUpdates()` 只报告哪些条目有新版，`store.updateHint(item)` 告诉你该怎么做——当前是 `"uninstall_reinstall"`：卸载后重装，安装路径会按版本漂移重新导入，取到市场当前版本。
- 卸载是**可重入**的：产物已不在算成功。部分失败时那些组件保持已安装、`ok: false` 且点名到组件，重试有意义。
- `store.setEnabled(item, false)` 对技能只翻一个目录层标记，返回 `code: "skill_disable_flag_only"`——技能语料没有启用状态，要让技能离开运行时只能 `uninstall`。这个 code 会原样透出，不会被吞。

规则（P0 实测结论）：

- 子进程固定 `--host 127.0.0.1 --no-open`；非回环一律拒绝（`isLoopbackUrl`）。
- 工具面不用改宿主自己的 `~/.agent-store/config.toml`：`env.AGENT_STORE_TOOLS` 直接指定，值是 JSON（形状同 `[tools]` 表），**整份替换**文件里的策略，`{}` = 全开。它只在 Store 宿主生效（桌面 / Web 宿主不采纳 `[tools]`），且子进程启动时读一次。

  ```ts
  const session = await launchClient({
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

不在本包范围内（`07-typescript-sdk.md` §非目标）：`config/*` 与 `skill/*` 写面。它们的**类型**在 `@flowy-agent-store/protocol`（wire 上存在的方法就有形状），但**没有** typed 方法——因为这些面跟着宿主自己的设置文件与技能目录走，是最易变的部分。这是可发现性边界，不是权限边界：`client.transport` 是公开的，服务端按磁盘归属判定可写性。要用就直连 `transport.request`，并自担变动。
