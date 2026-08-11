# Montage ↔ Canvas 互通

高保真单向物化（Montage Agent → Canvas）+ 显式写回（Canvas → Montage）。Canvas 不替代多阶段 pipeline / HITL；只投影资产并支持镜头级微调。

## 用户路径

1. Montage 工作区跑完关键 stage 后，点 **打开到画布**
2. 后端扫描项目 Creative IR → 拷贝媒体入 `video-canvas/media` → 写入节点图 `doc.json`（含 `alloCreative` / `alloMontage`）
3. Canvas 中微调 / 单镜重生成
4. 需要时点 **写回 Agent**（显式）：镜头媒体回写到 Montage 项目相对路径

## API

| Method | Path | 说明 |
|--------|------|------|
| POST | `/api/montage/projects/{id}/materialize-to-canvas` | 物化 → `{ project_id, montage_project_id, …, warnings, reused? }` |
| POST | `/api/montage/projects/{id}/sync-from-canvas` | body: `{ project_id, shots? }` |
| GET | `/api/montage/projects/{id}/files/{*path}` | 项目根下相对路径二进制（路径穿越安全；供预览 / `<img>` / `<video>`） |

媒体相对路径（Creative IR）落在 Montage 项目根下，例如 `assets/images/…`、`renders/final.mp4`。前端可用 `projectFileUrl(id, relPath)` 拼出上述 files URL。

## 物化内容

打开到 Canvas 会投影风格 / 角色 / 环境 / 分镜 / 镜头视频 / 成片等（以 `CreativeFilm` 为准）。合并成片走本机 `POST /api/video-canvas/media/concat`。

**幂等**：同一 Montage `project_id` 多次「打开到画布」复用同一画布项目，不会新建副本覆盖已有微调。

## 代码位置

| 层 | 路径 |
|----|------|
| Creative IR | `crates/agent/nomi-montage`（`creative`） |
| 物化 / 写回 | `crates/backend/nomifun-montage/src/materialize.rs` |
| 媒体入库 | `CanvasService::ingest_local_file` |
| 前端入口 | `WorkspacePage`「打开到画布」 |
| 溯源条 | `videoCanvas/lib/VimaxProvenanceBar.tsx`（Montage 溯源） |

## 非目标（有意不做）

- Canvas 接管 Montage 全量 pipeline / HITL
- Agent / Canvas 双向 live 同步
- 兼容旧 `/api/vimax` 物化路径
