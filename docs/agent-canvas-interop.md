# Agent ↔ Canvas 互通

高保真单向物化（Agent → Canvas）+ 显式写回（Canvas → Agent）。**不降低** ViMax 管线规划质量：Canvas 不替代多 Agent 规划，只投影资产并支持镜头级微调。

## 用户路径

1. Agent 工作区完成 plan / render 后，点 **打开到 Canvas**
2. 后端扫描 `.working_dir` → Creative IR → 拷贝媒体入 `video-canvas/media` → 写入节点图 `doc.json`（含 `alloCreative`）
3. Canvas 中微调 / 单镜重生成（自动注入 `FIXED SPEAKER VOICE`）
4. 需要时点 **写回 Agent**（显式）：镜头 mp4 回写 + ffmpeg 重拼接成片

## API

| Method | Path | 说明 |
|--------|------|------|
| POST | `/api/vimax/sessions/{id}/materialize-to-canvas` | 物化 → `{ project_id, …, warnings }` |
| POST | `/api/vimax/sessions/{id}/sync-from-canvas` | body: `{ project_id, shots?, reconcat? }` |

## 物化内容（对齐 Agent 多参考图链路）

打开到 Canvas 会投影：

| 节点 | 来源 | 连线角色 |
|------|------|----------|
| 风格 / 需求 | session style | → 角色 / 环境 / 道具 / 分镜 / 成片 |
| 角色定妆 | `character_portraits_registry` | → 分镜 · → 各镜头视频 |
| 环境板 / 道具板 | `world_assets_registry` | → 分镜 · → 各镜头视频 |
| 分镜 Script | storyboard + shot_descriptions | → 各镜头视频（row handle） |
| 连续末帧 | `shots/*/video_last_frame.png` | → 下一镜视频（Seedance Image 1） |
| 镜头视频 | `shots/*/video.mp4` | → 成片 |
| 成片 | `final_video.mp4` | concat 结果 |

合并成片走本机 `POST /api/video-canvas/media/concat`（与 Agent ffmpeg 同源），不再依赖 unpkg WASM。


## 代码位置

| 层 | 路径 |
|----|------|
| Creative IR | `crates/agent/nomi-vimax/src/creative/` |
| 物化 / 写回 | `crates/backend/nomifun-vimax/src/materialize.rs` |
| 媒体入库 | `CanvasService::ingest_local_file` |
| 前端入口 | `WorkspacePage`「打开到 Canvas」 |
| 溯源条 | `videoCanvas/lib/VimaxProvenanceBar.tsx` |
| Voice 注入 | `alloVimaxBridge.ts` + generation executor |

## 非目标（有意不做）

- Canvas 接管 ViMax 全量 plan/render 管线
- Agent / Canvas 双向 live 同步
- 用最低公分母 Schema 抹平 camera tree / voice bible
