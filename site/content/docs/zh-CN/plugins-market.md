# 插件与市场

Agent Store 原生支持**插件**与**插件市场**：插件是打包好的能力集合（专家、团队、技能、连接器、命令），市场则是这些能力的分发渠道。与运行时一致，插件体系同样遵循**本地优先**——市场只负责发现与分发，导入后的一切（快照、凭据、运行）都发生在本机。

## 1. 插件包含什么

一个插件（Plugin）可以携带以下组件，导入后成为 Catalog 中可复用的标准化定义：

| 组件 | 说明 |
| --- | --- |
| 专家（AgentDefinition） | 可复用的专家配置，在运行时通过 Preset 机制承载 |
| 专家团（AgentTeamDefinition） | 固定成员名册 + 协作规则 |
| 技能（SkillDefinition) | 原子能力，被 Agent 调用，不独立对话 |
| 连接器（ConnectorDefinition） | 与外部系统交互的受管能力，附凭据 Schema |
| 命令（CommandDefinition） | 用户可调用的提示/命令 |
| 钩子 / LSP | 生命周期钩子与语言服务器（元数据级） |

## 2. 市场从哪里来

市场来源在 [`~/.agent-store/config.toml`](/zh-CN/docs/configuration) 中声明，支持四种 `source_kind`：

| source_kind | source | 说明 |
| --- | --- | --- |
| `url` | HTTPS/HTTP 清单地址 | 建议同时提供目录枚举（`_files.txt`）以支持条目树镜像 |
| `github` | GitHub 仓库 | 从 GitHub 拉取市场清单 |
| `git` | Git 仓库地址 | 通过 Git 协议同步 |
| `directory` | 本地目录路径 | 直接指向本地市场/插件根 |

```toml
[default_marketplaces.workbuddy-experts]
source_kind = "url"
source = "https://market.example.com/experts/.codebuddy-plugin/marketplace.json"

[default_marketplaces.workbuddy-skills]
source_kind = "url"
source = "https://market.example.com/skills/.codebuddy-skill/marketplace.json"

[default_marketplaces.connectors]
source_kind = "url"
source = "https://market.example.com/connectors/.codebuddy-connector/connectors.json"
```

启动时运行时会拉取并解析这些市场清单，Web UI 的 **市场** 页面（顶部导航 / 页脚入口）即可浏览全部条目：专家、技能与连接器。

## 3. 导入：从市场到 Catalog

在市场中安装一个条目（`store/install-entry`）后，导入器（Importer）按固定顺序处理：

```text
1. 定位来源（市场清单条目 / 插件根 / 技能或连接器目录）
2. 解析 Manifest（plugin.json / marketplace.json / connectors.json）
3. 路径校验：拒绝 ../ 越界与符号链接逃逸
4. 复制到版本化不可变缓存，计算 content_digest
5. 生成 PluginSnapshot（组件清单 + 来源信息 + 兼容性报告）
6. 按组件类型产出标准化定义，注册到本地 Catalog
```

支持的来源格式：

- **CodeBuddy / WorkBuddy 插件**：`.codebuddy-plugin/plugin.json` + 组件目录
- **WorkBuddy Skill 市场**：`.codebuddy-skill/marketplace.json` + `skills/<slug>/`（单个含 `SKILL.md` 的目录同样支持）
- **WorkBuddy Connector 市场**：`.codebuddy-connector/connectors.json` + `connectors/<slug>/`

## 4. PluginSnapshot：不可变快照

导入的核心产出是 **PluginSnapshot**——来源内容的一次不可变镜像：

- 包含来源类型、来源 URI、声明版本、`content_digest`、组件清单与兼容性报告；
- 来源内容一旦变化，必须生成**新**快照，历史快照永不原地修改；
- 运行中的 Agent / Team 冻结其目标快照，不受 Catalog 后续更新影响；
- 历史运行可追溯到具体 snapshot、definition version 与 content digest。

未通过兼容性校验的组件会被标记状态而不是静默丢弃；许可证不明的资源不会进入公开分发。

## 5. 凭据与安全

- 导入连接器时**只建立凭据 Schema**，不读取真实密钥；
- 真实凭据只在运行时进入本地安全存储，Web / SDK 仅接触状态、账号标识与过期时间；
- 市场同步与导入全程不做远程执行，执行语义唯一归属本地 `allo` Runtime。

## 6. 自研 MCP Server / 自定义技能怎么接入

**自研 MCP Server 不需要走 Marketplace 注册接口。** 市场只是分发渠道；运行时接入 MCP 的统一入口是 MCP 配置（`mcp_servers`），有两条路径：

**路径 A：直接注册（自研 / 私有部署推荐）**

通过 MCP 配置接口按名注册，Web UI 的连接器管理页也是这套接口：

- `POST /api/mcp/servers` — 注册/更新一个 MCP Server（按名称 upsert）
- `POST /api/mcp/servers/import` — 批量导入
- `POST /api/mcp/test-connection` — 连接测试
- `/api/mcp/oauth/*` — 标准 OAuth（PKCE Loopback）登录

传输层支持三种，按你的服务器形态选择：

```jsonc
// 本地进程
{ "stdio": { "command": "./my-mcp-server", "args": [], "env": {} } }
// Streamable HTTP（远程推荐）
{ "http": { "url": "https://mcp.example.com/mcp", "headers": { "Authorization": "Bearer <token>" } } }
// SSE（旧式远程）
{ "sse": { "url": "https://mcp.example.com/sse", "headers": {} } }
```

API Key / 自定义鉴权直接写在传输层的 `headers`；标准 OAuth 由运行时负责登录、存储与请求注入。注册后在会话/Run 中绑定该 Server（`selected_mcp_server_ids`），运行时 `McpManager` 建连并把工具注入模型。

**通过 TypeScript SDK 接入**

SDK 的连接器客户端（`connector.list / get / status / test / authStart / authStatus`）是**只读 + OAuth 直通**的——App Server 协议没有「注册 MCP Server」的 WebSocket 方法。SDK 内的自研 MCP Server 接入走协议原生的**导入 → 安装**链路：把 Server 打包为连接器市场目录，`import/run` 导入为不可变 PluginSnapshot，`install/run` 安装时由运行时自动注册进 `mcp_servers`。

1. 写一个最小连接器市场目录：

   ```text
   my-mcp/
   └── .codebuddy-connector/
       └── connectors.json
   ```

   ```json
   {
     "name": "my-connectors",
     "version": "0.1.0",
     "connectors": [
       { "id": "my-mcp", "name": "My MCP", "type": "remote-mcp", "url": "https://mcp.example.com/mcp", "auth": "oauth" }
     ]
   }
   ```

2. SDK 会话内导入并安装（`import` / `install` 暂无子客户端封装，用 `transport.request` 透传协议方法）：

   ```ts
   import { launchClient } from "@flowy-agent-store/sdk";

   const session = await launchClient({ client: { name: "my-app", version: "0.1.0" } });

   // 1. 导入：生成不可变 PluginSnapshot（同 digest 重复导入幂等，返回 reused=true）
   const snap = await session.client.transport.request("import/run", {
     source_path: "C:/abs/path/to/my-mcp", // 本地目录绝对路径
     source_kind: "workbuddy-connector-market",
   });

   // 2. 安装：connector 组件自动注册进运行时 mcp_servers
   await session.client.transport.request("install/run", { snapshot_id: snap.snapshot_id });

   // 3. 查询目录并在 Run 中注入
   const connectors = await session.client.connectors.list();
   await session.client.runs.agent({
     agentId: "<agent-id>",
     goal: "……",
     mentions: [{ kind: "connector", id: connectors[0].id }],
   });

   await session.close();
   ```

3. 标准 OAuth 直接用 SDK 的 `connector.authStart(connectorId)` 发起、轮询 `connector.authStatus` 至 `authenticated`；
4. Run 时通过 `mentions` 注入：`{ kind: "connector", id }` 追加到 run 的 MCP 列表（须为已启用 Server）；技能则用 `{ kind: "skill", id }` 挂载。安装状态可用 `install/status` 查询、`install/enable` / `install/disable` 管理。

**路径 B：市场 / 插件分发（面向公开分发）**

希望别人能从市场一键安装时，把自研 MCP Server 打包为：

- **连接器市场条目**：`.codebuddy-connector/connectors.json` + `connectors/<slug>/`，发布到一个市场源（`source_kind` 支持 `url` / `github` / `git` / `directory`）；
- **插件级 MCP**：插件包内 `.codebuddy-plugin/` 附带 `.mcp.json`（`mcpServers` 字段）。

用户安装后同样落到 `mcp_servers`——两条路径最终殊途同归。注意 V1 的 OAuth 仅支持标准 PKCE Loopback，自定义 URI scheme、公网 relay 等复杂授权暂不支持。

**自定义 Skill**

- SDK / 协议接入：`import/run`（`source_kind: "workbuddy-skill-market"`，单个含 `SKILL.md` 的目录同样支持）→ `install/run`，随后 Run 中 mention 挂载；
- 本机接入：`POST /api/skills/import`（目录或 zip）导入为用户技能；
- 市场分发：按 `.codebuddy-skill/marketplace.json` + `skills/<slug>/` 布局发布到市场源。

## 7. 延伸阅读

- 市场条目浏览：[市场页面](/zh-CN/market)
- 市场源配置：[配置文件](/zh-CN/docs/configuration)
- 架构与导入分层：[架构说明](/zh-CN/docs/architecture)
