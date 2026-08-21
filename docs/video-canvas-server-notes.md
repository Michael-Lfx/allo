# Video Canvas 模式 — 服务端 / 本机说明

Canvas 模式（DEV）复用 **现有 flowy-cloud** 能力（与 Agent / nomi-vimax 同一套）：

- 模型列表：`/api/media/models`（含 **Seedance 2.0** 生视频、**Seedream 5.0** 生图等）
- 生图 / 生视频：本机 `nomifun-canvas` → `FlowyImage` / `FlowyVideo` → flowy-cloud

**不依赖** Creative Workshop；**不需要**再开一套云端任务中心。

---

## 本机 API（`nomifun-canvas`，`/api/video-canvas/*`）

| 方法 | 路径 | 说明 |
|------|------|------|
| GET/POST | `/projects` | 画布项目列表 / 创建 |
| GET/PATCH/DELETE | `/projects/{id}` | 详情 / 改标题 / 删除 |
| PUT | `/projects/{id}/doc` | 持久化画布文档 |
| GET | `/media` | 媒体列表 |
| POST | `/media/upload` | 上传素材 |
| POST | `/media/concat` | **本机 ffmpeg 拼接**多段视频 |
| GET | `/media/{id}` | 二进制 serve（auth-exempt） |
| DELETE | `/media/{id}` | 删除媒体 |
| POST | `/tasks` | 创建图/视频生成（Flowy） |
| GET | `/tasks/{id}` | 轮询 |
| POST | `/tasks/{id}/cancel` | **硬取消**（`CancellationToken` 中断 Seedance 轮询） |
| POST | `/llm/v1/chat/completions` | **最小 LLM 代理**（转发 Flowy OpenAI chat；Agent 工具循环在客户端） |

数据目录：`{data_dir}/video-canvas/`。

## 前端调用约定（与 Workshop / ViMax 对齐）

统一走 `httpBridge`：

| 能力 | 入口 | 说明 |
|------|------|------|
| JSON API | `httpRequest(method, '/api/...', body)` | `getBaseUrl()` + `buildBackendAuthHeaders` |
| 上传 | `XMLHttpRequest` + auth 头 | 同 `uploadCanvasMedia` / workshop `uploadAsset` |
| 媒体二进制 | `fetch(absoluteUrl, { headers: auth, credentials: 'omit' })` | 勿 `credentials: 'include'`（CORS `*`） |
| 生图/生视频 | `POST /api/video-canvas/tasks` | 经 `task-center` → `FlowyImage` / `FlowyVideo` |
| Agent 对话 | `streamCanvasChatCompletions` → `POST /api/video-canvas/llm/v1/chat/completions` | 工具循环在客户端；服务端只透传 chat |

**不要**：相对路径裸 `fetch('/api/...')`（桌面会打到 Vite）、`/api/ai/custom`、OpenAI `/responses`、影策 OSS 直连。

### Canvas Agent

- **业务逻辑（工具、画布 ops、会话 UI）在前端** `oc/components/canvas/canvas-assistant-panel.tsx`
- 服务端只做 Flowy `/chat/completions` 透传；文本模型来自 `modelProfile.resolve({ task: 'chat' })`，写入 OC `allo-chat` 渠道
- 桌面开发：LLM / 媒体请求必须打到 `http://127.0.0.1:{backendPort}/api/...`，不能相对打到 Vite `5173`
- 媒体 `fetch` 使用 `credentials: omit` + local-trust 头（后端 CORS 为 `*`，不能与 credentialed 请求共用）
- 图/视频参考媒体用本地 `resource:` / `/api/video-canvas/media` 即可，不必经过影策 OSS 用户缓存门禁
- 原 OC「影视 Agent 会话」依赖服务端拆解，**不迁到 allo 后端**；请用客户端 `canvas_*` 工具完成同样目标

### 生成映射（当前云端已支持）

| Canvas 操作 | 本机行为 |
|-------------|----------|
| 文生图 / 图生图 | Seedream 等 → `FlowyImage` |
| 文生视频 | Seedance 等 → `FlowyVideo`（t2v） |
| 图生视频 | 上游图作 `first_frame` / 参考图 → Seedance i2v |
| 运镜 | 提示词预设 + 同上视频 API |
| 拼接成片 | **本机** `concat_videos`（不调云） |

---

## 仍属产品增强（非阻塞云端）

| 项 | 说明 |
|----|------|
| 高级 edit op（extend / inpaint / replace…） | 待 flowy-cloud 明确契约后再开 UI；勿暴露空按钮 |
| TTS 配音节点 | 可接现有 `/api/tts`，结果写入 canvas media |
| 画布缩略图 | 可选：put doc 时异步出 JPEG |
| 与 `nomifun-creation` 统一任务审计 | 可选；当前独立任务表足够跑通 Canvas |

---

## 文件夹 / 时间线 / 字幕（client-doc-first）

**不新建后端路由**。全部落在现有不透明 `PUT /api/video-canvas/projects/{id}/doc` JSON 内（schema 仍为 `1`）。

| 能力 | Doc 契约 | 前端入口 |
|------|-----------|----------|
| 文件夹 | `nodes[]` 中 `type: frame` + `metadata.folder: { style, theme?, createdAt, themeCover? }`；子节点靠 `parentId`。**勿写** `assetFolderId`（不接影策 `/asset-folders`） | 添加节点菜单「文件夹」→ `createFolder` |
| 时间线 | 项目级 `doc.timeline?: TimelineProject`（`version: 2`, `tracks`, `clips`, `durationMs`） | 视频/音频悬停工具「时间线」 |
| 字幕 | 视频节点 `metadata.subtitleEntries / subtitleHighlights? / subtitleStyle? / subtitleUpdatedAt?`；SRT 客户端解析 | 视频悬停工具「字幕」 |

成片拼接仍走现有 `POST /media/concat`；时间线导出 wasm / 烧录字幕为后续增强，不要求新 API。

---

## 前端入口

- Agent：`/video-generation`
- Canvas（DEV）：`/video-generation/canvas`、`/video-generation/canvas/:id`
- **互通**：Agent 工作区「打开到 Canvas」→ `POST /api/vimax/sessions/{id}/materialize-to-canvas`（见 [agent-canvas-interop.md](./agent-canvas-interop.md)）

Canvas 编辑器为 **open-ai-canvas 完整 UX 移植**（Leafer 无限画布、节点面板、工具栏、连线），源码在 `ui/src/renderer/pages/videoCanvas/oc/`，经 `@oc/*` 别名接入。云端生成 / 媒体走本机 `/api/video-canvas`；影策原 Go 后端的积分、分享、领域项目等为本地 stub。
