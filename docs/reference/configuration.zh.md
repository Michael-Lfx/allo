# 配置参考

Flowy 读取的每一个参数与环境变量、它们的默认值，以及各自归属的文件。
所有取值均直接来自源码定义与实现。

Flowy 交付的是**一个**统一的 Rust 后端（`nomifun-app`，二进制 `nomicore`），以及三个面向不同部署形态的宿主模式：

- `nomifun-desktop` —— Tauri 桌面端外壳。在选定的 loopback 端口上以 `AuthPolicy::TrustLocalToken` 模式启动后端，并把每次启动生成的本地信任密钥注入自己的 WebView。
- `nomifun-web` —— 独立的 Web/服务端宿主。默认以**已鉴权（需登录）**模式启动同一个后端，并在同一端口上托管完整的 Flowy React SPA。
- `agent-store` —— 独立的 Agent Store 应用宿主。以**本地优先（默认无鉴权）**模式启动同一个后端，并在同一端口上通过嵌入式资源提供专属 Web UI，自带首次配置向导。

所有宿主与独立的 `nomicore` 进程共享底层的同一组配置面；各自的 CLI 仅用于覆盖各自拥有的那部分参数。

## `nomifun-web` 参数与环境变量

来源：[`apps/web/src/main.rs`](../../apps/web/src/main.rs)。

| 参数 | 环境变量 | 默认值 | 用途 |
|---|---|---|---|
| `--host` | `NOMIFUN_WEB_HOST` | `127.0.0.1` | 绑定的 IP。`0.0.0.0` 会接受 LAN/VPN/公网流量；大范围暴露前请先预置或完成首次设置。不解析主机名；非法输入将在启动阶段直接失败。 |
| `--port` | `NOMIFUN_WEB_PORT` | `8787` | TCP 端口。在同一个 socket 上提供 API、`/ws` WebSocket 与 SPA。 |
| `--data-dir` | `FLOWY_DATA_DIR` / `NOMIFUN_DATA_DIR` | 按用户的应用数据目录 | 后端数据目录（SQLite 数据库、智能体状态、日志、Bun 缓存）。默认是当前 channel 的宿主共享的按用户位置；stable 使用 `Flowy/Nomi`，dev 使用同级的 `Flowy/Nomi-dev`。环境变量按字面值生效（生产环境请使用绝对路径）。优先级：`--data-dir` > `FLOWY_DATA_DIR` > `NOMIFUN_DATA_DIR` > channel 默认值。 |
| `--dist` | `NOMIFUN_WEB_DIST` | `../../ui/dist` | 已构建 SPA 所在目录。在仓库之外部署时务必显式指定。 |
| `--api-only` | — | `false` | 仅提供后端/API 服务，不托管前端 SPA。专用于 Vite 前端热重载开发模式，防止静态构建包产生冲突。 |
| `--admin-user` | `NOMIFUN_ADMIN_USERNAME` | `admin` | 预置首位管理员时使用的用户名。管理员存在后将被忽略。 |
| `--admin-password` | `NOMIFUN_ADMIN_PASSWORD` | — | 在启动时预置首位管理员密码，跳过交互式设置。管理员存在后将被忽略。 |
| `--insecure-no-auth` | `NOMIFUN_WEB_INSECURE_NO_AUTH` | `false` | 危险。完全禁用鉴权（桌面式本地模式）。仅可用于 loopback 或完全受信任的私有网络。 |

布尔环境变量接受 `1`、`true`、`yes`、`on`（不区分大小写）。

## `agent-store` 宿主参数与环境变量

来源：[`apps/agent-store/src/main.rs`](../../apps/agent-store/src/main.rs)。

Agent Store 独立宿主将后端 API 与 Web UI（`web/dist`）打包为单二进制文件交付，同一端口提供服务，无需跨域与复杂网关。

| 参数 | 环境变量 | 默认值 | 用途 |
|---|---|---|---|
| `--host` | `AGENT_STORE_HOST` | `127.0.0.1` | 监听的主机地址。如需开放局域网访问请配合 `--auth` 使用。 |
| `--port` | `AGENT_STORE_PORT` | `8787` | 监听端口（同时提供 API、App Server WebSocket 与 Web UI）。传入 `--port 0` 开启 SDK 临时端口模式，实际地址将以 JSON 单行打印在 stdout。 |
| `--data-dir` | `FLOWY_DATA_DIR` / `NOMIFUN_DATA_DIR` | 按用户的应用数据目录 | 后端数据目录，与其它宿主保持相同的解析规则与排他锁机制。 |
| `--auth` | `AGENT_STORE_AUTH` | `false` | 危险警示：默认在本地以无鉴权模式运行（与桌面端一致）；显式启用 `--auth` 时将开启登录鉴权，并在首次访问时引导管理员账号设置。 |
| `--no-open` | — | `false` | 启动后不自动调用系统浏览器打开 Web UI。 |
| `--admin-user` | `NOMIFUN_ADMIN_USERNAME` | `admin` | 鉴权模式下首次启动的管理员用户名。 |
| `--admin-password` | `NOMIFUN_ADMIN_PASSWORD` | — | 鉴权模式下首次启动的管理员密码。 |
| `--log-level` | `NOMI_LOG_LEVEL` | `info` | 后端日志过滤指令（支持 tracing EnvFilter，如 `info,nomifun_mcp::oauth_service=debug`）。 |

子命令：

| 子命令 | 用途 |
|---|---|
| `init` | 首次运行交互式配置向导：生成 `~/.agent-store/config.toml` 并写入内置市场源（可选择配置初始模型供应商）。 |

## `nomicore`（后端）参数

来源：[`crates/backend/nomifun-app/src/cli.rs`](../../crates/backend/nomifun-app/src/cli.rs)。

下面是独立 `nomicore` 二进制对外暴露的参数。所有宿主均构造一个带默认值的 `Cli`，仅覆盖各自拥有的那部分——单独运行后端时这些参数完全适用。

| 参数 | 默认值 | 用途 |
|---|---|---|
| `--host` | `127.0.0.1`（`DEFAULT_HOST`） | 监听的主机地址。 |
| `--port` | `25808`（`DEFAULT_PORT`） | 监听端口。 |
| `--data-dir` | 按用户的应用数据目录 | 数据库 + 文件存储根目录。通过 clap 绑定 `FLOWY_DATA_DIR`，兼任 `NOMIFUN_DATA_DIR` 别名；两者未设置时解析当前 channel 的默认路径。 |
| `--work-dir` | （无） | 会话工作区目录。回退顺序：UI 中选择并持久化在 `dir-config.json` 的工作区 → `NOMIFUN_WORK_DIR` 环境变量 → 数据目录本身。 |
| `--app-version` | crate 版本 | 报告给扩展引擎用于做兼容性检查的宿主应用版本。 |
| `--local` | `false` | 独立 `nomicore` 的无鉴权本地模式。`nomifun-web --insecure-no-auth` 映射到同一策略。桌面外壳不使用该 flag，而是使用 `TrustLocalToken`。 |
| `--agent-store-config` | （按宿主而定） | Agent Store 配置文件绝对路径（通过 clap 绑定 `AGENT_STORE_CONFIG` 环境变量）。`nomifun-web` 默认为 `~/.agent-store/config.toml`。 |
| `--log-dir` | `<data-dir>/logs` | 滚动日志的目录。 |
| `--log-level` | `info` | 日志级别过滤。支持按 target 覆盖——例如 `info,nomifun_mcp=trace`。 |

子命令（供智能体 CLI 桥与诊断运维使用）：

| 子命令 | 用途 |
|---|---|
| `mcp-requirement-stdio` | AutoWork requirement 声明工具的 MCP stdio server。 |
| `mcp-knowledge-stdio` | 每会话 knowledge search 的 MCP stdio server。 |
| `mcp-gateway-stdio` | 平台 Gateway 工具的内部 stdio 传输；只接受宿主签发、带作用域、有效期和签名的能力声明。 |
| `mcp-open-stdio` | 暴露可靠 OS `open` 工具的 MCP stdio server。 |
| `mcp-computer-stdio` | 暴露 desktop computer-use 工具的 MCP stdio server。 |
| `mcp-browser-stdio` | browser-use 的带作用域 MCP stdio 代理；转发到主进程 `BrowserSessionHub`，不创建私有 Chromium 或 profile。 |
| `terminal-hook --event <kind>` | 一次性 terminal 生命周期 hook relay。 |
| `doctor` | 自检：填充智能体注册表，逐个探测 `$PATH` 上的每个 CLI，并打印一张按智能体维度的可用性表格。 |
| `tools` | 以 JSON 列出 Remote 能力名称与描述。 |
| `call <name> [json-args]` | 通过 `/v1` 调用运行中实例上的 Remote 能力。 |
| `backup --output <dir>` | **离线完整备份**：在获取排他锁后备份数据库、静态加密密钥、companion 文件以及受管会话工作区。必须输出到源目录之外。 |
| `restore --bundle <dir> --destination-data-dir <dir>` | **离线完整恢复**：将备份包恢复到全新/空目标数据目录中，自动轮换存储代际（storage-generation）。支持可选 `--destination-work-dir`。 |

## 共享环境变量

下列变量由后端读取，不论被哪个宿主嵌入。

| 环境变量 | 读取方 | 作用 |
|---|---|---|
| `FLOWY_DATA_DIR` | 所有宿主 | 后端数据目录的真值来源。在桌面端、Web 宿主与 `nomicore` 上均按字面值作为最终数据根。优先级高于 `NOMIFUN_DATA_DIR`。 |
| `NOMIFUN_DATA_DIR` | 所有宿主 | `FLOWY_DATA_DIR` 的兼容别名。优先级：`--data-dir` > `FLOWY_DATA_DIR` > `NOMIFUN_DATA_DIR` > channel 默认值。都未设置时回退至按用户默认目录。 |
| `FLOWY_HOME` / `NOMIFUN_HOME` | 智能体 / 工具链 | 显式指定 Flowy 根工作目录（未设置时回退到默认用户目录）。 |
| `NOMIFUN_WORK_DIR` | `nomicore` | `--work-dir`（按会话区分的工作区根）的回退值。优先级低于 UI 中持久化的工作区设置；若指向默认数据根或不存在目录会被安全忽略。 |
| `NOMIFUN_MANAGED_FETCH_MODE` | 桌面端后端 | 托管网页抓取回退模式。未设置或设为 `evidence-backed` 时启用带证据支撑的 PDF/JavaScript/空内容 MCP 回退；`off` 则回滚为纯本地抓取。 |
| `NOMIFUN_SSRF_ALLOW_CIDRS` | 出站抓取 (`nomifun-net::ssrf`) | 以逗号分隔的 SSRF 豁免 CIDR 网段，专用于将 DNS 解析映射到私网空间的代理隧道（如 sing-box IPv6 fake-IP `fc00::/18`）。默认已允许 `198.18.0.0/15` 与 `240.0.0.0/4`。 |
| `NOMIFUN_ENABLE_FREE_MODELS` | 所有宿主 | 保留的 `nomifun-free-model` 供应运维恢复开关。设为真值并重启后恢复相关服务、路由与 UI。 |
| `NOMIFUN_BUN_PATH` | 运行时 / Doctor | 显式指定 Bun 解释器的可执行文件绝对路径。未设置时自动在 `$PATH` 与系统默认安装目录探测。 |
| `NOMIFUN_EXTENSIONS_PATH` | 插件扩展加载器 | 扩展插件的自定义查找路径。 |
| `JWT_SECRET` | `nomifun-app` | 用于签发会话 JWT 的密钥。解析顺序见 [鉴权密钥解析](#鉴权密钥解析)。 |
| `NOMIFUN_HTTPS` | `nomifun-auth::CookieConfig` | 取真值时，会话与 CSRF cookie 会带上 `Secure` 标记和 `SameSite=Strict`。当应用通过 HTTPS 反向代理暴露时请打开。默认 `false` → `SameSite=Lax`。 |
| `SHELL` | 智能体引擎（Linux/macOS） | 智能体引擎派生子进程时使用的 shell。在 systemd 下的 Linux 服务器上请显式设置（系统账户通常没有 `$SHELL`）。 |
| `NOMIFUN_URL` | `nomicore call` | 调用 Remote capability 时使用的运行中实例 base URL。 |
| `NOMIFUN_COMPANION_TOKEN` | `nomicore call` | 访问 `/v1` Remote capability 路由的 companion access token。 |
| `SENTRY_DSN` | `nomicore` / 宿主 | 后端 panic 与 tracing 错误的可选 Sentry DSN。未设置则关闭 Rust 崩溃上报。不上报对话正文。 |
| `NOMI_LOG_LEVEL` | 宿主应用 | 宿主日志级别的环境变量覆盖通道（尤其适用于被 SDK 拉起、无法传 CLI 参数的子进程）。 |

## 前端构建变量

这些在 Vite 构建时打进 SPA。未设置密钥则对应 SDK 关闭；用户也可在「设置 → 使用分析」选择退出。

| 环境变量 | 读取方 | 作用 |
|---|---|---|
| `VITE_POSTHOG_KEY` | SPA 构建 | PostHog 项目 key。未设置则关闭产品分析。 |
| `VITE_POSTHOG_HOST` | SPA 构建 | PostHog 上报地址。默认 `https://us.i.posthog.com`。 |
| `VITE_SENTRY_DSN` | SPA 构建 | 渲染进程崩溃的 Sentry DSN。未设置则关闭 JS 崩溃上报。 |

## 后端常量

来源：[`crates/backend/nomifun-common/src/constants.rs`](../../crates/backend/nomifun-common/src/constants.rs)、`nomifun-file` 与 `nomifun-realtime`。
这些是编译期常量，不是环境变量——列在这里供运维了解相关限制上限。

| 常量 | 取值 | 用途 |
|---|---|---|
| `DEFAULT_HOST` | `127.0.0.1` | `nomicore` 的默认 `--host`。 |
| `DEFAULT_PORT` | `25808` | `nomicore` 的默认 `--port`。（Web 宿主与 Agent Store 将其覆写为 `8787`。） |
| `BODY_LIMIT` | `10 MiB` | 应用于每条路由的默认请求体大小限制。需要更大的路由（例如 `/api/fs/upload`）会安装自己的更大限制。 |
| `UPLOAD_MAX_SIZE` | `30 MiB` | 文件上传路由（`/api/fs/upload`）的上限。 |
| `MAX_REMOTE_IMAGE_SIZE` | `5 MiB` | 下载聊天中引用的远程图片时的上限。 |
| `COOKIE_NAME` | `nomifun-session` | 会话 cookie。 |
| `CSRF_COOKIE_NAME` | `nomifun-csrf-token` | CSRF cookie（不是 HttpOnly——JavaScript 需要读取它）。 |
| `CSRF_HEADER_NAME` | `x-csrf-token` | 与 CSRF cookie 值对应的请求头（Double Submit Cookie 模式）。 |
| `COOKIE_MAX_AGE_DAYS` | `30` | Cookie 的 `Max-Age`。 |
| `SESSION_MAX_AGE_SECONDS` | `30d` | JWT 有效期，与浏览器会话 Cookie 生命周期保持一致。 |
| `HEARTBEAT_INTERVAL` / `HEARTBEAT_TIMEOUT` | `30s` / `60s` | WebSocket 的心跳 ping/pong。 |

## 数据目录与工作目录的语义

- `data-dir` 存放 SQLite 数据库（`flowy-backend.db*`）、各智能体状态、Bun 缓存、日志文件，以及任何嵌入式扩展数据。把它当成普通数据库来对待——做好备份、限制权限。两个同时运行的后端共享它的情况已被机制性地阻止（见下面的服务器锁）。
- 所有宿主（`nomifun-desktop`、`nomifun-web`、`agent-store` 与 `nomicore` 二进制）都通过 `nomifun_app::cli::default_data_dir()` 解析默认目录。同一 build channel 的宿主共享它：stable 使用按用户的 `Flowy/Nomi` 目录（Windows 上的 `%LOCALAPPDATA%\Flowy\Nomi`、macOS 上的 `~/Library/Application Support/Flowy/Nomi`、Linux 上的 `$XDG_DATA_HOME/Flowy/Nomi`），非 stable channel 使用 `Flowy/Nomi-dev`、`Flowy/Nomi-beta` 等**同级目录**——channel 目录永远不嵌套在 stable 根之内。根脚本中的 `dev`、`dev:web`、`build:fast` 选择 dev；已安装应用、`serve:web` 与 release 构建保持 stable。需要把 stable 快照复制到开发目录时运行 `bun run seed:dev`；需要显式位置时则使用 `FLOWY_DATA_DIR`、`NOMIFUN_DATA_DIR` 或 `--data-dir`。
- 后端启动时（早于打开数据库）会对 `{data_dir}/server.lock` 取一把 OS 级**排他锁**。同一数据目录上的第二个后端进程会快速失败，错误信息会指出持有者（pid + 可执行文件名）并给出两条出路：关掉另一个实例，或用 `FLOWY_DATA_DIR` / `NOMIFUN_DATA_DIR` / `--data-dir` 给这一个指一个独立目录。锁是 advisory 的（经 `fs2` 走 `flock` / `LockFileEx`），进程退出或崩溃时由 OS 自动释放——残留的 `server.lock` 文件无害。`nomicore doctor` 与 `mcp-*` stdio 子命令不取这把锁（doctor 设计上允许与运行中的服务器并存）。
- `work-dir` 存放按会话区分的工作区。未设置时按以下顺序解析：`--work-dir` → UI 中选择并持久化在 `dir-config.json` 的工作区 → 非空的 `NOMIFUN_WORK_DIR` 环境变量 → 数据目录本身。继承到的 `NOMIFUN_WORK_DIR` 若指向默认数据根位置或已不存在的目录会被忽略——以防自动更新重启时残留的自导出值。会话会在 `<work-dir>/conversations/` 下创建子目录；删除会话同时删除其工作区。
- 所有宿主——包括桌面外壳——都把数据目录环境变量当作**最终数据根**，按字面值生效、不附加额外后缀，因此 Docker（`/data`）与 systemd（`/var/lib/nomifun`）部署不受影响。
- **历史数据自动迁移**：pre-0.3.4 遗留构建使用 `NomiFun/Nomi<suffix>`（或更早的 `<system temp>/nomifun-data/Nomi`）以及旧主库名 `nomifun-backend.db*`。新版升级后首次启动时：
  1. 数据库主文件与 sidecars 会自动从 `nomifun-backend.db*` 重命名迁移至 `flowy-backend.db*`；
  2. 既有的遗留目录数据集会被自动平滑迁移到 `Flowy/Nomi<suffix>`（一次性、抗崩溃、中断后下次启动续跑；若旧应用实例仍在运行则推迟到下次启动）；
  3. 数据库中持久化的绝对路径（知识库根目录、终端 cwd、自定义工作区）会在迁移后一次性安全改写。

## 鉴权密钥解析

`JwtService` 由单一密钥构造；`AppServices::from_config` 按以下顺序解析它：

1. 若已设置，使用 `JWT_SECRET` 环境变量。
2. 否则，使用 `installation_identity.owner_user_id` 所指向的安装所有者用户行中持久化的值。
3. 否则，生成一个全新的强随机密钥，并**持久化到数据库**供后续启动使用。

修改密码流会顺带轮换 JWT 密钥，使所有现有会话失效。

静态加密使用独立的持久密钥，存放在 `<data-dir>/encryption_key`。旧安装若还没有该文件，启动时会用当前解析到的 JWT 密钥派生并写入一次，以保证既有加密字段仍可读取；之后修改密码或轮换 JWT 密钥不会再改变数据加密密钥。

## TLS / HTTPS Cookie 处理

Flowy 自身不做 TLS 终止——请在前面放置负责 TLS 终止的反向代理（Caddy、nginx 等）。届时：

- 设置 `NOMIFUN_HTTPS=true`，使 cookie 带上 `Secure` 标记和 `SameSite=Strict`。否则浏览器会在 HTTPS 响应上拒收不带 `Secure` 的会话 cookie，登录看似会无声失败。
- `/ws` 上的 WebSocket 升级无需额外的请求头即可穿过任何符合标准的代理；Caddy 开箱即用。

可参考 [`guides/web-server-deployment.md`](../guides/web-server-deployment.md) 中完整的 Caddy + Docker 示例。

## 日志

- 所有日志同时写入 stdout（让 `journalctl`/`docker logs` 能捕捉到）以及 `<log-dir>/nomicore.log` 上的按日滚动文件。
- `--log-level` 接受完整的 [`tracing` `EnvFilter`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) 指令：一个全局级别，或一组以逗号分隔的按 target 覆盖项。

  示例：

  - `info` —— 全局 info。
  - `debug` —— 全局 debug。较啰嗦；适合短时复现。
  - `info,nomifun_mcp=trace` —— 默认 info，MCP 模块为 trace。
  - `warn,nomifun_conversation=info,nomifun_terminal=debug` —— 整体更安静；会话引擎为 normal/info；终端为 debug。

不存在另一套 `RUST_LOG` 通路——`--log-level`（或宿主中等价的环境变量 `NOMI_LOG_LEVEL`）是唯一的总开关。

每次后端启动还会把**日志目录与生效级别**写成日志文件的第一行，抓到的日志本身就能说明这次运行记录到了什么。

### Agent Store 宿主：配置文件与市场启动下载策略

还有两个环境变量常用于拉起宿主但无法传后端命令行参数的调用方（如 `@flowy-agent-store/sdk`）。
**所有解析后端 CLI 的宿主**（`agent-store`、`nomifun-web`、桌面壳）都会读取它们：

| 变量 | 作用 |
| --- | --- |
| `AGENT_STORE_CONFIG` | **本进程**要读取的 `config.toml` 绝对路径。只有显式 `--agent-store-config` 能覆盖它。未设置时各宿主保留自己的默认（`agent-store` 与 `nomifun-web` 为 `~/.agent-store/config.toml`；桌面壳**完全不注册**默认市场）。 |
| `AGENT_STORE_MARKET_DOWNLOAD` | `eager` \| `lazy` \| `none` —— **所有**默认市场源的启动策略，覆盖各源自己的 `download_on_start`。`lazy` 只注册不取包；`none` 连注册都不做。仅作用于本进程：**不写入配置文件**。 |

```ts
const harness = await launchHarness({
  client: { name: "my-app", version: "1.0.0" },
  configPath: "./my-config.toml", // → AGENT_STORE_CONFIG
  marketDownload: "lazy",        // → AGENT_STORE_MARKET_DOWNLOAD
});
```

`AGENT_STORE_CONFIG` 是“让拉起方能使用 per-source `[default_marketplaces.<id>] download_on_start`”的前提。它本身也是一个开关：在**本来解析不到配置文件**的宿主（桌面壳）上，把它指向一个文件就等于为该进程打开了默认市场注册**与**后台自动更新检查——也就是独立 `agent-store` 宿主默认所处的状态。

策略本身（`[marketplace] auto_update_interval_hours`、`entry_auto_update_kinds`）与运行期读写面（`market/settings` · `market/settings-set`）详见 Agent Store 专门文档：
[`docs/agent-store/37-market-download-policy.zh.md`](../agent-store/37-market-download-policy.zh.md) 与 [`docs/agent-store/18-marketplace-spec.zh.md`](../agent-store/18-marketplace-spec.zh.md) §9.2。

## 另见

- [Web 服务部署](../guides/web-server-deployment.md) —— 用 Docker、systemd、Caddy 运行 `nomifun-web`。
- [作为桌面应用运行 Flowy](../guides/desktop-app.md) —— 桌面端专属配置。
- [API 概览](./api-overview.zh.md) —— 配置完成并启动后，后端对外暴露了什么。
- [疑难排查](./troubleshooting.zh.md) —— 配置在运行时出错时的症状与修复方法。
