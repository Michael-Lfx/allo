# `@flowy-agent-store/protocol`

Versioned App Server Protocol 的纯类型包：线协议类型 + wire 错误，无运行时代码，不依赖 DOM/Node（`docs/agent-store/07-typescript-sdk.md §2.1`）。

- 唯一来源：`src/protocol.ts`、`src/errors.ts`。
- 语义来源：`docs/agent-store/10-public-contracts.md`；改动方法/错误码时三处同步更新。
- `web/src/lib/protocol.ts` 与 `web/src/lib/errors.ts` 只是 re-export 垫片， historical import 不动；新代码直接 `from "@flowy-agent-store/protocol"`。
- 实验性能力（Team 完整能力、事件 cursor 追平等 V2 项）在此包标 experimental，不进稳定导出（`docs/agent-store/12-sdk-packaging.md P2`）。
