# `@flowy-agent-store/client`

Transport 无关的 App Server 客户端（`docs/agent-store/07-typescript-sdk.md §2.2`）：`AppServerClient` + 7 个子客户端 + `Transport` 接口。所有业务方法只走注入的 `Transport`，包内无 HTTP、无 DOM、无 Node 依赖。

Web 专属（资产 `<img>` URL、`/api/fs/browse`、一次性 HTTP 注册）不在此包，由宿主 app 通过子类/组合提供——见 `web/src/lib/client.ts`。
