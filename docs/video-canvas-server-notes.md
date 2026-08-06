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

数据目录：`{data_dir}/video-canvas/`。

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

## 前端入口

- Agent：`/video-generation`
- Canvas（DEV）：`/video-generation/canvas`、`/video-generation/canvas/:id`

Canvas 编辑器为 **open-ai-canvas 完整 UX 移植**（Leafer 无限画布、节点面板、工具栏、连线），源码在 `ui/src/renderer/pages/videoCanvas/oc/`，经 `@oc/*` 别名接入。云端生成 / 媒体走本机 `/api/video-canvas`；影策原 Go 后端的积分、分享、领域项目等为本地 stub。
