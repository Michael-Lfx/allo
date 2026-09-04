# `@agent-store/sdk`

Node.js host for the Agent Store runtime (`docs/agent-store/12-sdk-packaging.md P1-3`): spawn `agent-store`, wait for its stdout readiness line, connect over loopback with `@agent-store/client`.

```ts
import { launchClient } from "@agent-store/sdk";

const session = await launchClient({
  client: { name: "my-app", version: "0.1.0" },
});
const store = await session.client.listStore();
await session.close();
```

规则（P0 实测结论）：

- 子进程固定 `--host 127.0.0.1 --no-open`；非回环一律拒绝（`isLoopbackUrl`）。
- 默认自带临时 `--data-dir` 并在 `close()` 删除；传自己的目录即表示独占——后端单实例锁会 fail-fast。
- 二进制定位：`bin` 参数 → `AGENT_STORE_BIN` → `PATH`；找不到直接报错（不下载，见 P2）。
- 就绪行 `protocol_version` 与 SDK 不一致时杀掉子进程并报错（含两端版本）。
