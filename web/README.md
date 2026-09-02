# allo 应用服务器 Web UI

面向 [Agent Store 应用服务器协议](../docs/agent-store/05-allo-app-server-protocol.md) 的独立 Web UI。它是一个协议消费方——只使用公开的应用服务器契约（JSON-RPC 2.0 WebSocket + 辅助 HTTP 接口），绝不触碰 allo 内部实现。

## 快速开始（离线演示）

```bash
cd web
bun install

# 终端 1：模拟应用服务器（假引擎，真实协议形态）
bun run mock

# 终端 2：Web UI
bun run dev
```

打开 http://localhost:5174，在连接设置中填写 WebSocket 地址后连接。Provider ID 和模型名称可以留空：后端会从 `~/.agent-store/config.toml` 解析 `default_model`（如 `opencode/mimo-v2.5-free`），并自动把 `[providers.<name>]` 注册进 Allo（凭据加密入库，幂等复用）。发送第一条消息会自动创建一个持久化的 presetless Nomi 对话；mock 服务器会推送用户消息、turn 状态和 assistant 回复事件。

## 连接到真实的应用服务器

应用服务器路由挂载在已认证的应用路由器之下（受实例所有者鉴权中间件保护的 `/api/app-server/...`）。当浏览器能够访问真实后端时（本地信任的开发模式或已正确鉴权的部署环境）：

- WebSocket URL：`ws://<host>/api/app-server/ws`
- 可选的 Bearer 令牌：在 WS 握手时以 `?token=…` 形式追加（浏览器 WebSocket 无法设置请求头）。后端是否认可查询令牌取决于其鉴权接线方式；在禁用鉴权的开发模式下无需令牌。

聊天 UI 只使用 WebSocket 协议表面；SDK 中的 HTTP 辅助接口（`registerWorkspace` 等）保留给命令行/测试客户端使用。

UI 会执行完整的生命周期：connect（连接）→ `initialize` → 协议版本检查 → `initialized` → ready（就绪），随后拉取对话、模型目录与 `workspace/list`。聊天使用 `conversation/create`、`conversation/send`、`conversation/messages`、`conversation/cancel`、`conversation/update`（重命名/模型/思考等级）、`conversation/delete` 与 `conversation/subscribe`；工作区使用 `workspace/list`、`workspace/create`（校验并注册 owner 指定的本地文件夹绝对路径）与 `workspace/revoke`（移除/注销工作区）。侧栏按 `workspace_id` 把会话分组；每个工作区标题右侧的“+”直接在该工作区新建并打开会话（无需再输入路径），其旁的移除按钮可隐藏该工作区及其会话——会话不会被删除，重新注册同一路径后自动重新显示。全局“新对话”仍提供“选择已有工作区或输入绝对路径”的流程。每条发送都带独立幂等键，历史从服务端持久化记录恢复，实时事件按连接内 sequence 去重。旧的 `agent/run` 执行协议仍保留在 SDK 中，但聊天 UI 不依赖 preset-backed execution。

上下文占用（顶部指示器与 composer 芯片）使用服务端在每次 `TurnCompleted` 实测并持久化的 `context.usage` 快照（`used_tokens` / `window_tokens` / `percent`），通过 `conversation/get` 与实时事件投影到 `ConversationView.context_usage`；未上报或窗口未知时明确显示“暂不可用”，绝不做字符数估算。

输入区为自动增长的多行编辑器（默认 3 行、随内容增长有视口上限），`Enter` 发送、`Shift+Enter` 换行，中文 IME 组合阶段不会触发发送。

## 测试

```bash
bun scripts/smoke.ts                          # 启动一个临时模拟服务器并驱动整个 SDK 流程
bun scripts/smoke.ts --real                   # 连接真实 App Server 后端（默认 127.0.0.1:8787）
```

模拟模式断言项包括：握手 + 能力协商、工作区注册、聊天创建/列表/持久化历史、带幂等键的消息发送、实时 `message.created` / `message.thinking`（流式思考，按 `message_id` 聚合、可折叠）/ `message.delta` / `turn.status` 投递，以及旧 run 协议的收敛和结构化错误码。

真实模式对生产装配验证同一套协议表面。启动方式：

```bash
cargo run -p nomifun-web -- --port 8787 --api-only --insecure-no-auth --data-dir <临时目录>
```

真实后端验证覆盖握手、能力协商、工作区注册、技能/连接器目录和聊天协议表面。真实 Nomi 回复要求目标 Allo 数据目录中已注册、启用且具备可用凭据的 Provider/模型；有 `~/.agent-store/config.toml` 时 App Server 会自动注册其 provider 并解析 `default_model`，无需先用 Allo UI 手动配置（2026-09-01 已验证：`opencode/mimo-v2.5-free` 自动注册、消息持久化、模型真实流式响应）。Team、Skill 自动注入和 MCP 都由 App Server 聊天创建/运行时边界禁用。旧的 `agent/run` 启动成功路径仍依赖 `builtin-office` 预设；缺资产时它会以 `not_found` 干净失败。

## 技能与连接器目录

侧栏的「技能与连接器」入口（仅在服务端 `initialize` 能力协商开启 `skills`/`connectors` 时显示）打开 Agent Store 目录视图（`src/components/CatalogView.tsx`）：

- **技能**：`skill/list` / `skill/get` 展示公共摘要（名称/版本/来源/兼容性/所需连接器/指令摘要）；原始 `SKILL.md` 正体和内部路由规则不返回。
- **连接器**：`connector/list` / `get` / `status` / `test` 展示类型、传输摘要、命名空间化工具、授权状态与合并后的状态徽标（`connected` 仅当授权就绪且最近探测成功——TC-CONN-002）；OAuth 连接器支持「授权」（`connector/auth/start`，浏览器回调由可信主机处理，UI 轮询 `connector/auth/status`）、「取消授权」与「测试连接」。

目录数据只来自 App Server WebSocket 协议；UI 不接触真实 Token。

## 目录结构

```text
src/lib/
  transport / client / conversations / runs / skills / connectors / errors
                    类型化的协议客户端（React 零依赖）
  activity.ts       消息/活动载荷解码器；NOISE_ACTIVITY_KINDS 是生命周期噪音规则的唯一出处
  conversation-events.ts  会话流的纯 reducer（消息合并 + 乐观发送对账 + context.usage 投影）
  errors.ts         AppServerError + formatError
src/ui/format.ts    跨组件共享的展示格式化（shortId / formatTokens / modelName / providerLabel / modelChipLabel / modelKeyToSelection）
src/store/appStore.ts  单 zustand store：接管全部应用状态（连接/会话列表/会话流/草稿/UI 开关/模型/工作区/对话框）与所有动作；组件直接 useAppStore 订阅，无 prop drilling
src/components/
  App.tsx           Codex 风格的单 Agent 持久化聊天壳（薄壳：DOM ref + 三个 effect + 渲染）
  CatalogView.tsx   Agent Store 技能/连接器目录视图
  Sidebar / Topbar / MessageList / Composer   外壳四段（直接消费 store）
  IconButton.tsx / ContextIndicator.tsx / ModelPicker.tsx   复用小部件
  dialogs/          Settings / NewChat / Rename / Delete / WorkspaceRemove 五个对话框（自门控，直接消费 store）
  messages/         MessageItem / ActivityItem / ThinkingItem / TipsItem / ToolCallItem / EmptyStates
scripts/mock-server.ts   支持聊天事件、旧 run 协议与技能/连接器目录的 Bun mock 服务
scripts/smoke.ts         端到端冒烟测试
```

`src/lib` 模块刻意保持零依赖（不依赖 React，也不依赖 allo 内部），以便日后可按 `docs/agent-store/07-typescript-sdk.md` 抽取为可复用的 `@agent-store/client` 包。聊天 UI 不提供 Team、审批、附件或原始 `work_dir` 控件。
