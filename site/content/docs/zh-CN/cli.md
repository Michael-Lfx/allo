# 命令行用法

`flowy-agent-store` 是打包后的单文件运行时入口：后端与内嵌 Web UI 在**同一端口**服务。绝大多数交互在浏览器工作台完成，命令行只负责启动参数——直接运行即可，无子命令：

## 启动 Web UI

```bash
flowy-agent-store
flowy-agent-store --port 8787
```

默认监听 `http://127.0.0.1:8787/`，启动后自动在默认浏览器打开工作台。

| 参数 | 说明 | 默认 |
| --- | --- | --- |
| `--port <n>` | 监听端口（API 与内嵌 Web UI 共用） | `8787` |
| `--host <ip>` | 监听地址，仅本机保持 `127.0.0.1` | `127.0.0.1` |
| `--data-dir <dir>` | 后端数据目录（数据库与存储） | 每用户的 Flowy/Nomi 目录 |
| `--auth` | 开启登录模式；不加则为本地可信模式（免登录） | 关闭 |
| `--no-open` | 启动后不自动打开浏览器 | 自动打开 |
| `--admin-user <name>` | 认证模式下首启创建的管理员用户名 | `admin` |
| `--admin-password <pw>` | 认证模式下首启创建的管理员密码；不传则由首个访问工作台的人交互创建 | 无 |
| `-h, --help` / `-V, --version` | 查看帮助 / 版本 | — |

## 常用示例

```bash
# 端口被占用时换一个（前端按页面地址自动连接，无需手动改设置）
flowy-agent-store --port 8788

# 局域网内其他机器访问（建议同时加 --auth）
flowy-agent-store --host 0.0.0.0 --auth

# 需要登录的多人/开放网络场景，预置管理员账号
flowy-agent-store --auth --admin-user admin --admin-password <pw>

# 指定数据目录 / 服务器上不打开浏览器
flowy-agent-store --data-dir /data/agent-store --no-open
```

## 环境变量

命令行参数也可用环境变量代替：`AGENT_STORE_HOST`、`AGENT_STORE_PORT`、`AGENT_STORE_AUTH`（`1`/`true`/`yes`/`on` 即开启）、`NOMIFUN_DATA_DIR` / `FLOWY_DATA_DIR`（数据目录）、`NOMIFUN_ADMIN_USERNAME` / `NOMIFUN_ADMIN_PASSWORD`（认证模式首启建号）。

## 说明

- 端口固定：被占用时直接报错退出，请关闭占用该端口的其他实例（桌面应用或之前的前后端进程），或换一个 `--port`。
- 前端零配置：内嵌 Web UI 按页面地址自动连接同源后端，换 `--port` / `--host` 后无需手动改地址；设置页仍可手动覆盖。
- 数据目录有独占锁：默认与桌面端共享同一份状态，同时运行会快速失败，这是防双写的保护，不是故障。
- 非回环地址（`--host 0.0.0.0`）且未加 `--auth` 时，所有能连上该端口的人都拥有完整主机权限，请确保可信网络或开启登录。
- 导入、运行与状态查询都在 Web UI 工作台里完成。命令行与 Web UI 共享同一个 App Server 协议边界，不会直接访问内部数据库或凭据存储。
