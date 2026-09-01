# 媒体与创作域（Workshop / 视频生成 / 模型调用层）

> **最后维护：** 2026-09-01 · 核对基准：源码（nomi-vimax 终态 hook + 第一方遥测）
> 文档性质：现行架构文档（新建，基于源码逐项核对）

本域覆盖 Flowy 的所有"生成媒体"能力：创意工坊画布、ViMax 视频管线、视频生成
Canvas 模式，以及它们共同依赖的统一多模态模型调用层。共 8 个 crate：

| Crate | 层 | 职责 |
| --- | --- | --- |
| [`nomifun-model-invoke`](../../crates/backend/nomifun-model-invoke/) | 后端库 | 统一多模态调用层（无自有 HTTP 面） |
| [`nomifun-creation`](../../crates/backend/nomifun-creation/) | 后端 | 生成任务队列 + 状态机 |
| [`nomifun-workshop`](../../crates/backend/nomifun-workshop/) | 后端 | 创意工坊：无限画布 + 资产 |
| [`nomifun-vimax`](../../crates/backend/nomifun-vimax/) | 后端 | ViMax HTTP 面（`/api/vimax/*`） |
| [`nomifun-canvas`](../../crates/backend/nomifun-canvas/) | 后端 | 视频生成 Canvas 模式（DEV） |
| [`nomifun-media`](../../crates/backend/nomifun-media/) | 后端 | 媒体设置 / 积分 / 工作流历史 |
| [`nomi-vimax`](../../crates/agent/nomi-vimax/) | 引擎 | ViMax 视频生成管线 |
| [`nomi-media`](../../crates/agent/nomi-media/) | 引擎 | 媒体生成引擎与 agent 工具 |

## 统一模型调用层 `nomifun-model-invoke`

P1 多模态重构的产物（设计稿：
[`specs/2026-07-28-multimodal-model-provider-redesign.zh.md`](../specs/2026-07-28-multimodal-model-provider-redesign.zh.md)）。

- **类型化任务**：`TaskRequest::{ImageGeneration, ImageEdit, VideoGeneration,
  SpeechSynthesis, ChatText, Embedding, Asr, Rerank}` →
  `TaskOutcome::{Done(TaskResult), Pending(JobHandle)}`；统一错误货币
  `InvokeError/InvokeErrorKind`。
- **声明式鉴权**：`AuthScheme = bearer | token | header_key:<name> |
  query_key:<param> | volc_voice(MultiHeader)`，传输层支持多 key 轮换。
- **协议适配接缝**：`ProtocolAdapter` trait（`id()/supports()/submit()/poll()`）
  + 不可变 `AdapterRegistry`；内置约 17 个适配器（openai.* 族、gemini、deepgram、
  ark、dashscope、minimax、zhipu、xai、siliconflow、volc_voice 等）。
  `(platform, task) → protocol` 的路由表 **默认拒绝**（deny-by-default）。
- **目录解析**：消费 `IProviderRepository` / `IProviderModelRepository` /
  `IProviderConnectionRepository`，AES-GCM 解密凭证后组装调用。
- **消费者**：`nomifun-creation`（图像/视频）、`nomifun-shell`（STT/TTS）、
  `nomifun-robot`（ASR/TTS）、`nomifun-ai-agent`（provider 健康探测）。

## 创作任务队列 `nomifun-creation`

工坊画布生成节点背后的引擎，供应商无关：

- 状态机 `queued → running → succeeded | failed | canceled`；
- 并发上限：全局信号量 **10** + 每 provider 信号量 **3**；
- 取消先写终态再触发 per-task `CancellationToken`（worker 永不覆盖 `canceled`；
  成功与取消竞态时回滚资产）；
- 异步协议：提交 → 序列化 `JobHandle` 存入 `remote_task_id` → 轮询循环
  （2.5 s 间隔，600 s 预算；重启后按开机时间续算预算）；
- 启动对账 `ensure_boot_ready`：可选 Workshop 前置 → 审计 manifest → 带 remote
  handle 的 running 任务恢复续跑，其余 live 任务判 `interrupted`；并清理 sink 侧
  孤儿资产；
- `AssetSink` trait 由应用层以 workshop 实现（避免 crate 依赖成环）；
- 表 `creation_tasks`（v3 baseline `001_v3_baseline.sql` 内建）；路由 `/api/creation/tasks*`。

已知限制：`v2v` capability 可解析但恒定返回 `unsupported_capability`。

## 创意工坊 `nomifun-workshop`

无限画布 AI 视觉创作工作区：

- 索引行在 SQLite（`workshop_canvases` / `workshop_assets`），画布正文
  （`canvas.json`，上限 8 MB）与资产二进制（上限 64 MB）落盘于
  `{data_dir}/workshop/canvases/{id}/` 与 `{data_dir}/workshop/assets/`（含缩略图）；
- 路由 `/api/workshop/canvases*`、`/assets*`（含 upload）、`/collections/rename`；
  公开免鉴权面仅 `GET /api/workshop/files/{asset_id}` 与 canvas 缩略图；
- Agent 操作入口：`AgentOp` / `AddNodeSpec` / `PendingOp`（pending-ops + ack）。

**现状**：后端完全可用；前端 `/workshop` 路由在 `Router.tsx` 中**有意搁置**
（页面保留在树中但未注册路由）。

## ViMax 视频管线

- 引擎 [`nomi-vimax`](../../crates/agent/nomi-vimax/)：idea2video /
  novel2video / script2video / action2video / script_film / script_scene_split /
  cameo_bind 七条管线，14 个子 agent（编剧、分镜师、封面等）；会话索引与归档为
  **纯文件存储**（`{data_dir}/vimax/.vimax/sessions.json` +
  `.working_dir/<id>/`，无 SQL）；垂直技能包与 skill-hub；creative IR +
  `build_canvas_document` 供画布物化。
- HTTP 面 [`nomifun-vimax`](../../crates/backend/nomifun-vimax/)：
  sessions CRUD/import/plan/revise/render/status/cancel/export、artifacts、cameos、
  action-assets、`materialize-to-canvas` / `sync-from-canvas`（与 Canvas 双向同步，
  链接存 `vimax_session_links.json`）、tv-show 发布面、skills / skill-hub 面。
- 前端路由 `/video-generation`、`/video-generation/:sessionId` 已上线。
- **成片终态遥测**：`VimaxService` 在 Render job 结束（`Succeeded` / `Failed` / `Cancelled`）以及 `interrupt_all` 时仅对当时 `Rendering` 的会话发出 `film_*`（中断映射为 `film_cancelled`）。Plan→Idle 与 Plan 失败不报。hook 由 `nomifun-vimax` 接到 `CloudService::upload_video_growth_events`；`event_id = video:{name}:{session_id}` 与 UI 一致，服务端 UNIQUE 去重。属性含 workflow / 模型 / `credits_consumed` / `duration_ms` / `error_code` / `failure_channel`。增长口径见 [cloud-billing.zh.md](cloud-billing.zh.md)「第一方产品遥测」。

## 视频生成 Canvas 模式 `nomifun-canvas`

独立于 workshop 的第二条画布线（lib.rs 标注 **DEV**）：

- projects / media（上传、concat）/ tasks / cancel，另有原始 chat-completions
  透传代理 `/api/video-canvas/llm/v1/chat/completions`；
- 生成经 `FlowyVimaxServices` 走 nomi-vimax 的 Flowy 后端；拼接用 ffmpeg；
- 文件存储 `{data_dir}/video-canvas/`（projects/、media/、链接表）；
  上限 8 MB doc / 256 MB media；公开面仅 `GET /media/{media_id}`；
- 前端 `/video-generation/canvas`、`/video-generation/canvas/:id`。

## nomi-media ↔ nomifun-media

- 引擎侧 [`nomi-media`](../../crates/agent/nomi-media/)：plan/run/status/cancel
  agent 工具、workflow runner/store/templates、长视频规划、积分；由
  `wire_flowy_media(registry, config, data_dir)` 接线 —— **当前只注册
  image_generate 单发工具**，video/workflow 工具被 nomi-vimax UI 取代而跳过；
  未配置 flowy provider 时整体 no-op。
- 后端侧 [`nomifun-media`](../../crates/backend/nomifun-media/)：
  `/api/media/settings`、`/credits`（余额 / 打卡 / `usage-by-turn` 按回合积分明细）、
  `/models`、`/workflows/history`；工作流历史 JSON 在 `{data_dir}/media/workflows/`。

## 关系图

```
/workshop (UI 搁置) ──▶ /api/workshop/* (nomifun-workshop)
        └─ 生成节点 ──▶ /api/creation/* (nomifun-creation)
                              │ invoke
                              ▼
                      ModelInvokeService ──▶ 各 provider HTTP
/video-generation ──▶ /api/vimax/* (nomifun-vimax) ──▶ VimaxService(nomi-vimax)
        Canvas 模式 ──▶ /api/video-canvas/* (nomifun-canvas) ──┘（materialize 双向）
nomi-media 工具 ◀── nomifun-ai-agent 接线；nomifun-media 出设置/积分/历史
```
