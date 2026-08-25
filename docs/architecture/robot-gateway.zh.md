# 机器人网关域（Robot Gateway）

> **最后维护：** 2026-08-24 · 核对基准：commit `d791691c6` ·
> 文档性质：现行架构文档（新建，基于源码逐项核对）

[`nomifun-robot`](../../crates/backend/nomifun-robot/) 让 LAN 内运行小智
（xiaozhi）固件的实体机器人成为桌面伙伴的**物理化身**。`RobotGateway::serve`
按 `RobotLinkSource` 跑接受循环，每条链路 spawn 独立 `session::run_session` 任务。
宿主接线在 [`nomifun-app/src/robot_wiring.rs`](../../crates/backend/nomifun-app/src/robot_wiring.rs)；
加载失败只降级（桌面无机器人功能启动），绝不阻断 boot。

## 协议：OTA 发现 + WebSocket

- 固件经 HTTP `POST /robot/ota` 上报设备信息；**OTA 响应是固件获知宿主地址的唯一
  通道**——返回 `ws://<ip>:<port>/robot/v1` + bearer token（永远不含 mqtt）。
- LAN 监听端口优先 25808；Docker 可用 `ROBOT_ADVERTISE=<ipv4>[:<port>]` 覆盖广播地址。
- 媒体帧：JSON 文本帧 + 二进制 Opus 帧；上行 16 kHz、下行 24 kHz 单声道、60 ms 帧；
  Opus 经静态链接 libopus；VAD 用 Silero-ONNX（能量检测兜底）。
- 视觉照片走普通 HTTP `POST /robot/v1/vision/explain`（8 MB 上限）。

## 为什么直接依赖 `nomi-*`

管线只见 trait（`SpeechServices` ASR/TTS/vision + `CompanionTurnDispatcher`）：

- ASR/TTS 走 `nomifun-model-invoke::ModelInvokeService`（非 chat 解析算法）；
- 视觉 one-shot **绕过 invoke 层直连 `nomi_providers::LlmProvider`**（invoke 层
  当前仅文本，需满足固件 30 s 上限）—— 这就是 `nomi-config` / `nomi-providers` /
  `nomi-types` 三个无条件直接依赖的由来；
- 聊天本身仍经 dispatcher 进 conversation runtime（不裸调 LLM）；
- agent 工具经 loopback MCP 代理 `127.0.0.1:<ephemeral>/robot-mcp/{robot_id}`
  下达设备，per-boot bearer 鉴权（`mcp_proxy.rs`）。

## 绑定与管理

- 激活码认领 → `companion_id` 绑定；PATCH 支持改绑 / 解绑（`routes/admin.rs`）。
- 机器人线程是**普通会话**：`conversations.extra.robot_session=true` + `robot_id`；
  人设注入"机器人体"提示段；双方剥离情绪标记。
- 公开设备面：`nest("/robot", device_router)` 挂在 CSRF 之后（设备无 cookie，
  OTA 铸造的 bearer token 鉴权）；管理面 `/api/robots*` 在 owner 鉴权内。
- **无数据库表**：`RobotRegistry` / `RobotStatusRegistry` 全内存，token 每次 OTA
  上报轮换；唯一 DB 触点是会话侧（迁移 `032_robot_stage_direction_backfill.sql`）。
- 前端：伙伴工作台 RemoteTab 的 `RobotConnectSection` + `useRobotStatuses`；
  会话列表按机器人分组命名。无独立设置页。
