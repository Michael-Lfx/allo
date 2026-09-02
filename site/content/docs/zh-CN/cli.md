# 命令行用法

`flowy-agent-store` 是打包后的单文件运行时入口。绝大多数交互在浏览器工作台完成，命令行负责启动与少量本地操作。

## 启动 Web UI

```bash
flowy-agent-store serve
flowy-agent-store serve --port 8787 --open
```

| 参数 | 说明 |
| --- | --- |
| `--port` | App Server 监听端口，默认 `8787` |
| `--open` | 启动后在默认浏览器打开工作台 |
| `--host` | 监听地址，默认 `127.0.0.1`（仅本机） |

## 导入

```bash
flowy-agent-store import ./my-plugin.codebuddy-plugin
```

导入器会校验路径、计算内容摘要（digest），并生成兼容性报告（compatible / manual-review / unsupported）。

## 运行（可选，命令行亦可触发）

```bash
flowy-agent-store run --agent software-company.architect
flowy-agent-store run --team software-company
```

运行请求经由本地 App Server 的版本化协议提交；实际执行发生在 `allo` Runtime。工作流、重试与 replan 由 Runtime 内部规划器驱动。

## 状态查询

```bash
flowy-agent-store status
flowy-agent-store list agents
```

> 命令行与 Web UI 共享同一个 App Server 协议边界，不会直接访问内部数据库或凭据存储。
