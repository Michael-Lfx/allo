# OpenMontage → allo Agent 模式迁移实施方案

> 状态：**已落地**（机制忠实 / Canvas 独立 / 媒体走 Flowy；剩余为产品增强与端到端打磨）  
> 原则：向 OpenMontage **机制与质量**看齐；ViMax Agent 整删；控制面与工具面用 **Rust** 实现  
> **已确认产品决策（2026-08-11）：**  
> 1. **AGPL**：不拷贝上游 MD/YAML/代码；**语义等价原创重写** skills/YAML/schemas（质量优先，不降级）  
> 2. **TV Show**：视频生成**公共模块**，Agent / Canvas / 未来同级模式共用（长期建设）  
> 3. **Canvas**：OpenMontage 侧对应的是 **Backlot 观察板** → 做进 Agent 工作区；现有 **Creation Canvas 完全独立**、互不影响  
> 4. **同级模式**：数字人（avatar）等从 OpenMontage 能力提取为与 Agent 平级的视频生成模式，持续丰富  
> 关联：`docs/agent-canvas-interop.md`（Montage ↔ Canvas）

---

## 0. 目标与非目标

### 0.1 目标

1. 移除 `/video-generation` 下基于 ViMax 的 Agent 模式（管线、API、UI、数据协议全删）。
2. 在 allo 内建成与 OpenMontage **机制对等** 的 Agent 制片系统：
  - Pipeline YAML 状态机
  - Markdown stage-director / meta / creative / core skills
  - Artifact JSON Schema + Checkpoint
  - Executive Producer 编排（Rust LLM tool-calling，顶替 IDE coding agent）
  - Capability-first tools + selector
  - HITL、reviewer、delivery_promise、slideshow_risk、cost 治理
  - Project 磁盘布局与 Backlot 可观察状态语义
3. LLM / 生图 / 生视频继续走 **当前 Flowy 服务端 API**（`nomifun-cloud` / `nomi-media-backends`）。
4. Canvas（创作模式）保留；通过新的 Montage↔Canvas 物化桥互通。



### 0.2 非目标（首期明确不做）


| 项                                                    | 原因                          |
| ---------------------------------------------------- | --------------------------- |
| 100+ 云厂商 SDK 1:1 搬迁                                  | 统一收敛到 Flowy                 |
| 本地 GPU 生成（WAN/Hunyuan/CogVideo/LTX local/SadTalker…） | 桌面产品体积与运维成本过高               |
| 用 Canvas 重做 Backlot                                  | Canvas 是创作面；观察/审批在 Agent UI |
| 允许产品态任意执行用户侧 Python                                  | 安全面不可接受                     |
| 兼容 `/api/vimax` 或旧 `.working_dir` 会话                 | 你已授权整删                      |




### 0.3 成功标准（Definition of Done）

- [x] 用户路径上不再出现 ViMax / idea2video|script2video|novel2video / `/api/vimax`
- [x] 至少 **6 条 production pipeline** 的 YAML+skills 可加载；其中 **≥2 条端到端可出片**（见 Phase 门禁）
- [x] Artifact / checkpoint 通过与 OpenMontage 同构的 JSON Schema 校验
- [x] HITL：`awaiting_human` → 用户批准/打回 → 续跑
- [x] Compose 支持 `ffmpeg`；`remotion`/`hyperframes` 至少一条可跑（Phase 2）
- [x] `materialize-to-canvas` / `sync-from-canvas` 可用
- [x] Canvas 生图/生视频不依赖 `nomi-vimax`

---



## 1. 机制忠实：规格对齐表

OpenMontage 没有可移植的「Python Agent Runtime」。要对齐的是下面这张表。


| OpenMontage 规格                    | allo Rust 落点                                           | 忠实度要求                                                      |
| --------------------------------- | ------------------------------------------------------ | ---------------------------------------------------------- |
| IDE coding agent = EP             | `nomi-montage` Orchestrator（Flowy Chat + tools）        | 决策在 LLM；阶段闸门在 Rust                                         |
| `pipeline_defs/*.yaml`            | `nomi-montage/assets/pipeline_defs/` + `pipeline` 模块   | **原样迁入** + schema 校验                                       |
| `skills/**`                       | `assets/skills/`，按 stage 注入                            | **原样迁入**；Flowy 能力差异用补丁 skill 覆盖                            |
| `schemas/**`                      | `assets/schemas/` + runtime 校验                         | **原样迁入**                                                   |
| `styles/*.yaml`                   | `assets/styles/`                                       | **原样迁入**                                                   |
| `AGENT_GUIDE.md`                  | system contract 资源                                     | 保留 Rule Zero / HITL / runtime 锁定 / Decision Contract       |
| `lib/checkpoint.py`               | `checkpoint` 模块                                        | API 语义对齐 `CANONICAL_STAGE_ARTIFACTS`                       |
| `BaseTool` + registry             | `tools::{Tool, Registry}`                              | 字段：name/tier/capability/provider/runtime/schemas/fallbacks |
| selectors                         | `image_selector` / `video_selector` /（后）`tts_selector` | 打分逻辑对齐 `lib/scoring.py`                                    |
| providers                         | `flowy_image` / `flowy_video` / `flowy_chat`           | **替换** 多厂商，不保留 fal/veo/kling 直连                            |
| `video_compose` 三 runtime         | Remotion / HyperFrames / FFmpeg 子进程                    | 合同对齐 `edit_decisions.render_runtime`                       |
| Backlot BoardState                | Agent UI + `GET .../board-state`                       | 状态形状对齐，可不搬 FastAPI UI                                      |
| CostTracker                       | `cost` 模块 ↔ 积分/配额                                      | estimate→reserve→reconcile 语义                              |
| delivery_promise / slideshow_risk | `governance` 模块                                        | 规则对等，禁止静默降级                                                |
| `projects/<id>/` 布局               | `{data_dir}/montage/projects/<id>/`                    | **目录约定对齐**                                                 |




### 1.1 编排语义（必须等价）

每个 stage：

```text
读 director skill → 调 tools → 写 artifact(s)
→ meta/reviewer（≤N 轮）→ 写 checkpoint
→ (若 human_approval) awaiting_human → 人批/打回
→ 下一 stage 或 send-back
```

EP 约束（来自 pipeline YAML `orchestration`）：

- `max_revisions_per_stage`
- `max_send_backs`
- `max_wall_time_minutes`
- `budget_default`（映射积分）
- `default_checkpoint_policy`: `guided | manual_all | auto_noncreative`



### 1.2 产品化唯一故意偏差


| OpenMontage                                     | allo                                 |
| ----------------------------------------------- | ------------------------------------ |
| Agent **写 Python** 调 `registry.get().execute()` | Agent **只走已注册 Tool**（JSON schema 入参） |
| Runtime 不持有 LLM key                             | Runtime **通过 Flowy** 调 Chat          |
| 重依赖本地可选 GPU                                     | 默认云 API；GPU 工具不注册                    |


偏差目的：安全、可测、可打包；**不改变** pipeline/skills/artifacts/HITL 合同。

---



## 2. 目标架构

```text
ui/pages/videoGeneration          ui/pages/videoCanvas (保留)
  Agent 工作区 + Board 轨              无限画布创作
        │                                    ▲
        │ /api/montage/*                     │ materialize / sync
        ▼                                    │
nomifun-montage ─────────────────────────────┘
        │
        ▼
nomi-montage
  assets/   (YAML/MD/JSON Schema/styles — 来自 OpenMontage)
  pipeline/ checkpoint/ orchestrator/ tools/ governance/ creative/
        │
        ├── nomi-media-backends ──► nomifun-cloud (LLM/Image/Video)
        └── (optional) Node sidecar: remotion-composer / npx hyperframes
```



### 2.1 新建 / 替换 crate


| Crate                 | 动作           | 职责                                                                            |
| --------------------- | ------------ | ----------------------------------------------------------------------------- |
| `nomi-media-backends` | **新建**（先于删除） | 从 `nomi-vimax` 上提：`FlowyChat/Image/Video`、traits、`media_local`、aspect/quality |
| `nomi-montage`        | **新建**       | OpenMontage 核心运行时                                                             |
| `nomifun-montage`     | **新建**       | axum `/api/montage/`* + materialize                                           |
| `nomi-vimax`          | **删除**       | 管线全部移除                                                                        |
| `nomifun-vimax`       | **删除**       | HTTP 全部移除                                                                     |


Workspace：`allo/Cargo.toml` 增删 path 依赖；`nomifun-app` 装配切换。

### 2.2 `nomi-montage` 模块划分

```text
crates/agent/nomi-montage/
├── assets/                          # include_dir! 或运行时读安装目录
│   ├── AGENT_GUIDE.md
│   ├── pipeline_defs/*.yaml
│   ├── schemas/**/*
│   ├── skills/**/*
│   └── styles/*.yaml
├── src/
│   ├── lib.rs
│   ├── config.rs                    # 对齐 config.yaml 子集
│   ├── error.rs
│   ├── paths.rs                     # projects/<id>/...
│   ├── events.rs                    # events.jsonl
│   ├── pipeline/
│   │   ├── loader.rs                # ↔ pipeline_loader.py
│   │   ├── manifest.rs
│   │   └── stage.rs
│   ├── checkpoint/
│   │   ├── mod.rs                   # ↔ checkpoint.py
│   │   ├── schema.rs
│   │   └── canonical.rs             # CANONICAL_STAGE_ARTIFACTS
│   ├── artifacts/
│   │   ├── validate.rs              # JSON Schema
│   │   └── names.rs
│   ├── styles/
│   │   └── playbook.rs
│   ├── governance/
│   │   ├── scoring.rs
│   │   ├── delivery_promise.rs
│   │   ├── slideshow_risk.rs
│   │   └── cost.rs
│   ├── tools/
│   │   ├── contract.rs              # BaseTool 等价
│   │   ├── registry.rs
│   │   ├── selectors/
│   │   ├── flowy/
│   │   ├── ffmpeg/
│   │   ├── compose/                 # video_compose 路由
│   │   ├── remotion/
│   │   ├── hyperframes/
│   │   ├── artifact_io.rs
│   │   └── checkpoint_tools.rs
│   ├── orchestrator/
│   │   ├── ep.rs                    # Executive Producer loop
│   │   ├── stage_runner.rs
│   │   ├── prompts.rs               # 注入 guide/skills
│   │   ├── approvals.rs             # HITL
│   │   └── preflight.rs             # provider menu
│   ├── project/
│   │   ├── mod.rs                   # project.json CRUD
│   │   ├── board_state.rs           # Backlot-equivalent
│   │   └── export.rs                # .nomimontage
│   ├── creative/                    # → Canvas IR
│   └── service.rs                   # 对外服务门面
```



### 2.3 项目磁盘布局（对齐 OpenMontage）

```text
{data_dir}/montage/
  library.json                       # 可选：项目索引
  projects/<project-id>/
    project.json
    artifacts/*.json
    assets/{images,video,audio,music}/
    renders/
    pipeline/checkpoint_*.json
    history/
    events.jsonl
    decision_log.json                # 或 artifacts/decision_log.json
```

旧路径 `{data_dir}/vimax/` **不再读写**；不做迁移工具（除非产品另行要求）。

---



## 3. 资产迁入清单（从 OpenMontage 原样拷贝）

源根：`migrants/OpenMontage/`  
目标：`allo/crates/agent/nomi-montage/assets/`

### 3.1 P0（Phase 1 必须）


| 类别              | 源路径                    | 数量  | 说明                                              |
| --------------- | ---------------------- | --- | ----------------------------------------------- |
| 合同              | `AGENT_GUIDE.md`       | 1   | 可附 `AGENT_GUIDE.flowy.md` 补丁（只写 Flowy 能力差）      |
| Pipelines       | `pipeline_defs/*.yaml` | 13  | 含 smoke；启用策略见 §6                                |
| Schemas         | `schemas/**/*.json`    | 全套  | artifacts 19 + checkpoint/pipeline/styles/tools |
| Meta skills     | `skills/meta/*`        | 11  | 治理，不可缺                                          |
| Core skills     | `skills/core/*`        | 6   | ffmpeg/remotion/hyperframes…                    |
| Creative skills | `skills/creative/*`    | 35  | 含 prompting/*                                   |
| Pipeline skills | `skills/pipelines/*`   | 103 | 与 YAML `skill:` 路径一致                            |
| Styles          | `styles/*.yaml`        | 5   | playbooks                                       |
| Index           | `skills/INDEX.md`      | 1   | 查找表                                             |


**不要**迁入：`tools/**/*.py`、`lib/**/*.py`、`.agents/skills` 全量（体积大；按需摘录技术要点到 `skills/core` 补丁）、Backlot Python UI。

### 3.2 Remotion 子项目（Phase 2）

- 拷贝 `remotion-composer/` → `allo/crates/agent/nomi-montage/remotion-composer/`（或 `apps/montage-remotion/`）
- 打包策略单列（§9.3）：开发依赖本机 Node；发布评估 sidecar / 可选组件



### 3.3 许可与合规门禁（开工前）

OpenMontage 使用 **AGPLv3**（见其 `LICENSE`）。在合并任何源代码/指令资产进闭源桌面分发前：

1. 法务书面确认嵌入 MD/YAML/Schema/Remotion 子树的合规路径（开源双许可、内部源公开、或语义级重写）。
2. 在 repo 保留 `THIRD_PARTY_NOTICES` 与资产来源映射表。
3. 若法务否决「原文拷贝」，则 Phase 0 增加「语义等价重写 skills/YAML」工单（工期显著增加）。

**未过合规门禁不得合入主分支。**

---



## 4. ViMax 拆除与依赖上提



### 4.1 拆除顺序（强制）

```text
Step A  上提 backends + media_local → nomi-media-backends
        改 nomifun-canvas 依赖；跑 Canvas 生成/concat 烟测
Step B  落地 nomi-montage + nomifun-montage + /api/montage
        前端 Agent 切新 API（可先暗门 feature flag）
Step C  前端删 ViMax 工作区；home agent skills 换 pipeline 列表
Step D  重做 materialize/sync；Canvas 字段 vimax→montage
Step E  删 nomifun-vimax、nomi-vimax、vimax 测试、旧文档/i18n stages
Step F  清理 workspace Cargo、侧栏 recents、.nomivimax
```



### 4.2 必须先上提的符号


| 现位置                            | 新位置建议                                 |
| ------------------------------ | ------------------------------------- |
| `nomi-vimax/src/backends/*`    | `nomi-media-backends`                 |
| `nomi-vimax/src/media_local/*` | `nomi-media-backends`                 |
| `aspect` / `video_quality`     | `nomi-media-backends` 或 `nomi-config` |
| `VimaxError`                   | `MediaBackendError`                   |


`nomifun-canvas/src/generate.rs`、`service.rs`（concat）改为只依赖新媒体 crate。

### 4.3 前端 / 路由拆除面


| 项           | 路径                                                                                |
| ----------- | --------------------------------------------------------------------------------- |
| Agent UI    | `ui/.../pages/videoGeneration/WorkspacePage.tsx` 及 cameo/storyboard/artifact 专用组件 |
| API client  | `videoGeneration/api.ts` / `types.ts` → 重写为 montage                               |
| Home skills | `idea2video|script2video|novel2video` → OpenMontage pipeline ids                  |
| Router      | 保留 `/video-generation` 与 `/video-generation/canvas/:id`；`:sessionId` 绑 Montage    |
| 装配          | `nomifun-app` router/state/services 去 vimax、挂 montage                             |
| 测试          | 删 `nomifun-app/tests/vimax_cameo_e2e.rs`                                          |
| 文档          | 删或改写 `docs/vimax-tv-show-api*.md`；重写 `agent-canvas-interop.md`                    |
| 归档          | `.nomivimax` → `.nomimontage`                                                     |




### 4.4 TV Show

现挂在 `/api/vimax/tv-show/*`。处置二选一（实施前定案）：

- **A（推荐）**：迁到 `/api/montage/tv-show/`*，发布包改为导出 Montage `projects/<id>` 快照  
- **B**：Phase 1 暂时下线入口，Phase 3 再接

---



## 5. API 设计（`/api/montage`）

对齐 OpenMontage 项目生命周期，而不是 ViMax 的 `plan|render` 二元模型。

### 5.1 核心资源


| Method     | Path                                               | 说明                                                     |
| ---------- | -------------------------------------------------- | ------------------------------------------------------ |
| GET        | `/api/montage/pipelines`                           | 可用流水线（来自 YAML + stability + 本地能力过滤）                    |
| GET        | `/api/montage/pipelines/{name}`                    | manifest 详情（stages、skills、approval 默认）                 |
| GET        | `/api/montage/provider-menu`                       | preflight 能力菜单                                         |
| GET/POST   | `/api/montage/projects`                            | 列表 / 创建（topic、pipeline、playbook、预算、模型三元组）              |
| GET/DELETE | `/api/montage/projects/{id}`                       | 详情（含 checkpoint 摘要）/ 删除                                |
| POST       | `/api/montage/projects/{id}/start`                 | 启动 EP（从当前 stage 续跑）                                    |
| POST       | `/api/montage/projects/{id}/cancel`                | 取消                                                     |
| GET        | `/api/montage/projects/{id}/status`                | 运行状态（轮询）                                               |
| GET        | `/api/montage/projects/{id}/events`                | SSE（对齐 Backlot events）                                 |
| GET        | `/api/montage/projects/{id}/board-state`           | BoardState（阶段/ artifact/ cost/ storyboard）             |
| GET        | `/api/montage/projects/{id}/artifacts`             | artifact 列表                                            |
| GET/PUT    | `/api/montage/projects/{id}/artifacts/{name}`      | 读/人工改 artifact（schema 校验）                              |
| POST       | `/api/montage/projects/{id}/approvals`             | `{ stage, decision: approve|reject|send_back, note? }` |
| POST       | `/api/montage/projects/{id}/reference-video`       | 参考视频入口（分析任务）                                           |
| POST       | `/api/montage/projects/{id}/export`                | `.nomimontage`                                         |
| POST       | `/api/montage/projects/import`                     | 导入                                                     |
| POST       | `/api/montage/projects/{id}/materialize-to-canvas` | → Canvas                                               |
| POST       | `/api/montage/projects/{id}/sync-from-canvas`      | ← Canvas                                               |




### 5.2 创建请求（示例）

```json
{
  "title": "品牌片概念",
  "pipeline": "cinematic",
  "prompt": "……",
  "style_playbook": "flat-motion-graphics",
  "checkpoint_policy": "guided",
  "models": { "chat": "…", "image": "…", "video": "…" },
  "output": { "aspect": "16:9", "resolution": "1080p", "fps": 24 },
  "budget_credits": 1200,
  "reference_video_path": null
}
```



### 5.3 状态机（项目）

```text
idle → running → awaiting_human → running → … → succeeded
                 ↘ failed / cancelled
```

Stage status：`pending | in_progress | awaiting_human | completed | failed`（+ UI `stalled`）。

---



## 6. Orchestrator 实施规格



### 6.1 为什么独立于主会话 `nomi-agent`

视频制片会话需要 **stage-scoped tool allowlist、长时 job、磁盘 checkpoint、专用 HITL UI**。  
复用 `nomi-agent` 的思路（`Tool`、skill 加载、approval、AGENTS.md 注入），但跑在 `nomi-montage::orchestrator`，**不**把成片任务塞进主对话 workspace。

### 6.2 EP Loop（伪代码）

```rust
fn run_project(project_id) {
  preflight_provider_menu();
  inject(AGENT_GUIDE + pipeline EP skill);
  loop {
    let stage = checkpoint.current_stage()?;
    let director = load_skill(stage.skill);
    let tools = registry.allowlist(stage.tools_available + meta_tools);
    run_llm_tool_loop_until_stage_exit(director, tools, caps);
    validate_canonical_artifact(stage)?;
    maybe_reviewer_rounds(max=2)?;
    write_checkpoint(completed | awaiting_human)?;
    if needs_human(stage) { pause(); wait_approval(); apply_send_back_or_continue(); }
    advance_or_finish()?;
  }
}
```



### 6.3 Tool 合同（Rust）

对齐 `BaseTool` 字段：

- 身份：`name`, `version`, `tier`, `capability`, `provider`
- 运行：`runtime` (`Local` / `Api` / `Hybrid`), `stability`
- 合同：`input_schema`, `output_schema`
- 治理：`fallback_tools`, `resource_profile`, `estimated_cost`

统一返回：

```rust
struct ToolResult {
  ok: bool,
  artifacts: Vec<ArtifactRef>,  // 文件路径或 artifact 名
  message: String,
  cost: CostDelta,
  model: Option<String>,
  meta: serde_json::Value,
}
```

每次执行 append `events.jsonl`（供 Board / SSE）。

### 6.4 Phase 1 Tool 清单（必须注册）


| Tool                                   | 映射                                                                       |
| -------------------------------------- | ------------------------------------------------------------------------ |
| `write_artifact` / `read_artifact`     | 磁盘 + schema                                                              |
| `checkpoint_write` / `checkpoint_read` | checkpoint API                                                           |
| `image_selector` → `flowy_image`       | Flowy 生图                                                                 |
| `video_selector` → `flowy_video`       | Flowy 生视频                                                                |
| `video_stitch`                         | ffmpeg（来自 media_local 扩展）                                                |
| `video_compose`                        | Phase1 仅 `ffmpeg` runtime；其它 runtime 返回明确「不可用」而非静默降级                     |
| `ffprobe` / `extract_last_frame`       | 本地分析小工具                                                                  |
| `decision_log_append`                  | 决策轨                                                                      |
| `cost_estimate` / `cost_reconcile`     | 预算                                                                       |
| `web_search`                           | 若已有 `flowy_web::SearchProvider` 则接；否则 research stage 标记能力缺失并引导换 pipeline |




### 6.5 Phase 2+ Tools


| Tool                           | 说明                      |
| ------------------------------ | ----------------------- |
| `remotion_render`              | `npx remotion render …` |
| `hyperframes_compose`          | `npx hyperframes …`     |
| `tts_selector` / `flowy_tts`   | **依赖服务端能力**             |
| `music_*` / `audio_mixer`      | 同上                      |
| `scene_detect` / `transcriber` | ffmpeg 简化版或侧车           |
| `export_bundle`                | publish                 |


未实现的 YAML `tools_available` 项：registry 标记 `unavailable`，preflight 与 EP 必须 **显式告知**，禁止假装成功（对齐 delivery_promise）。

### 6.6 Skill 注入策略

- System：压缩版 `AGENT_GUIDE` + 当前 `orchestration.skill`（EP）
- 每 stage：完整 director MD + `review_focus` + `success_criteria`（来自 YAML）
- 按需：`meta/reviewer`、`meta/checkpoint-protocol`、`meta/animation-runtime-selector`
- Flowy 差异：追加 `skills/meta/flowy-capability-overlay.md`（自建，不来自上游）说明仅有的 image/video/chat 与积分模型



### 6.7 模型调用

- Chat：`nomifun_cloud` chat completions（支持长 context；注意 stage 间 transcript 压缩）
- Vision：参考视频/分镜质检时用 multimodal（若模型 catalog 支持）
- 禁止在 orchestrator 外散落直接 HTTP

---



## 7. Governance 对齐


| 模块                 | 行为                                                 |
| ------------------ | -------------------------------------------------- |
| `scoring`          | 七维权重与 OpenMontage 一致；cost/latency 映射积分与 p95        |
| `delivery_promise` | compose 前锁定 promise；禁止 motion→slideshow 静默降级       |
| `slideshow_risk`   | ≥ 阈值阻断 compose，逼回 assets/edit                      |
| `cost`             | warn / hard cap；超 `single_action_approval` 触发 HITL |
| `reviewer`         | advisory，默认最多 2 轮                                  |


单元测试：直接移植/改写 OpenMontage 对 promise & slideshow_risk 的边界用例（Rust）。

---



## 8. Canvas 互通（保留创作模式）



### 8.1 新 Creative IR（替换 ViMax IR）

从 Montage 工程读取：


| 来源                              | 物化到 Canvas   |
| ------------------------------- | ------------ |
| `style_playbook` + brief        | 风格/需求节点      |
| `script` / `scene_plan`         | 分镜 Script 节点 |
| `asset_manifest` 图片             | 角色/场景/道具媒体节点 |
| `asset_manifest` / renders 分镜视频 | 镜头视频节点       |
| `edit_decisions`                | 时间线注释 / 连接关系 |
| `renders/final.mp4`             | 成片节点         |




### 8.2 字段重命名


| 旧（ViMax）                                 | 新                                            |
| ---------------------------------------- | -------------------------------------------- |
| `source_vimax_session_id`                | `source_montage_project_id`                  |
| `vimax_session_links.json`               | `montage_project_links.json`                 |
| `metadata.alloVimax` / `alloCreative`    | `alloMontage`                                |
| `VimaxProvenanceBar` / `alloVimaxBridge` | `MontageProvenanceBar` / `alloMontageBridge` |


不做旧字段长期兼容（可一次性读失败即忽略）。

### 8.3 文档

重写 `docs/agent-canvas-interop.md`：标题改为 Montage ↔ Canvas；删除 camera_tree / portraits_registry 假设。

---



## 9. 前端实施方案



### 9.1 首页（`/video-generation`）

保留 `agent | creation` 分段：

- **creation**：不动（Canvas gallery）
- **agent**：
  - Pipeline 选择器（读 `/api/montage/pipelines`），展示 stability
  - Playbook 选择
  - Checkpoint policy
  - 模型/画幅/预算（复用现有 pickers，去 ViMax 类型）
  - 提交 → `POST /projects` → `start` → 进入 `/video-generation/:projectId`



### 9.2 Agent 工作区（整页替换）

按 Backlot 信息架构，而非 ViMax 分镜板：

1. **Stage Rail**：阶段列表 + 状态色 + HITL 徽标
2. **Main**：当前 stage 的 artifact 编辑器（JSON form / Markdown 预览）+ 媒体 filmstrip
3. **Decision / Cost rail**：`decision_log` + 消耗
4. **Live activity**：SSE events
5. **Approval bar**：批准 / 打回 / 备注
6. **Actions**：继续、取消、导出、打开到 Canvas

删除：Cameo cast、camera tree、ViMax ProgressTimeline 专用文案（可借鉴交互，不保留领域模型）。

### 9.3 i18n

- 重写 `videoGeneration.stages.*` → OpenMontage stage 名  
- `create.skills.*` → 各 pipeline 文案  
- 删除 planning/rendering 二元状态，改为 `running|awaiting_human|…`

---



## 10. 分阶段交付计划



### Phase 0 — 合规与脚手架（约 3–5 天）

- [ ] 法务结论落地  
- [ ] 建 `nomi-media-backends`，Canvas 切过去，删对 `nomi-vimax` backends 的依赖  
- [ ] 建空 `nomi-montage` / `nomifun-montage`，挂路由健康检查  
- [ ] 资产拷贝脚本 + schema 烟测  
- [ ] 本文档评审定稿；TV Show 选 A/B  

**门禁：** Canvas 生成/concat 回归绿；合规通过。

### Phase 1 — 机制 MVP（约 3–5 周）

范围：

- pipeline loader + checkpoint + artifact validate + events  
- EP orchestrator + HITL API + BoardState  
- Tools：Flowy image/video/chat + ffmpeg stitch/compose(ffmpeg) + artifact/checkpoint/cost  
- 迁入全部 assets（skills/yaml/schemas）  
- **端到端启用：** `cinematic` + `framework-smoke`（再加 `hybrid` 若时间够）  
- 前端新工作区最小可用  
- 首页 agent 模式切 pipeline  
- **用户路径移除 ViMax**（即便旧 crate 临时还在仓库，也不可达）

**门禁：**

- smoke pipeline 全绿  
- cinematic：从一句 prompt 到 `renders/final.mp4`（允许无 Remotion）  
- HITL 打回/批准可测  
- `/api/vimax` 404



### Phase 2 — 成片语言 + Canvas 桥（约 2–4 周）

- Remotion sidecar + `video_compose` 路由  
- HyperFrames（可选并行）  
- delivery_promise / slideshow_risk 强制嵌入 compose  
- materialize / sync + provenance UI  
- `animated-explainer` 端到端（依赖 Remotion）  
- 导出/导入 `.nomimontage`  
- 删除 `nomi-vimax` / `nomifun-vimax` crate 本体与残余引用

**门禁：** explainer 出片；Canvas 往返；仓库无 `vimax` 符号。

### Phase 3 — 能力扩展（持续）

- 其余 production：`animation`、`screen-demo`、`avatar-spokesperson`  
- TTS/music（服务端就绪后）  
- research `web_search` 强化  
- TV Show 接 Montage 包（若选 A）  
- beta pipelines 按优先级：`talking-head`、`podcast-repurpose`…  
- 可选 Python/ML 侧车：WhisperX、CLIP（仅 documentary）

---



## 11. 测试策略


| 层      | 内容                                                                     |
| ------ | ---------------------------------------------------------------------- |
| 单元     | schema validate、checkpoint 迁移、scoring、slideshow_risk、promise           |
| 合同     | 每个 YAML 的 skill 路径存在；canonical artifact 可解析                            |
| 工具     | flowy_* mock HTTP；ffmpeg 用小夹具 mp4                                      |
| 编排     | `framework-smoke` 全自动；cinematic 用录制 LLM fixture 或 `--offline-scripted` |
| HTTP   | `/api/montage` axum 测试                                                 |
| E2E UI | 创建→等 awaiting_human→批准→出片（可先 headless API）                             |
| 回归     | Canvas 任务生成 + concat 不被破坏                                              |


建议在 `nomi-montage/tests/fixtures/` 放入最小合法 artifact 样例（可从 OpenMontage tests 改写）。

---



## 12. 配置与运维



### 12.1 配置映射

OpenMontage `config.yaml` → allo：


| OM                  | allo                                       |
| ------------------- | ------------------------------------------ |
| `llm.*`             | Flowy chat model（会话创建时传入 / 用户偏好）           |
| `budget.*`          | 积分 + `MediaGenConfig` / 服务端配额              |
| `checkpoint.policy` | 项目级 `checkpoint_policy`                    |
| `output.*`          | 现有画幅/分辨率/fps pickers                       |
| `paths.*`           | `{data_dir}/montage/...` + embedded assets |




### 12.2 可观测性

- `events.jsonl` + SSE  
- orchestrator tracing span：project/stage/tool/cost  
- 失败时保留 checkpoint，支持 `start` 续跑



### 12.3 并发

- 每项目单 writer（Mutex / job token）  
- 全局视频生成闸门保留（现 FlowyVideo Semaphore=1 或可配置）

---



## 13. 风险与缓解


| 风险                    | 影响                 | 缓解                                                                 |
| --------------------- | ------------------ | ------------------------------------------------------------------ |
| AGPL 合规               | 阻塞合入               | Phase 0 法务；最坏语义重写 skills                                           |
| 无 TTS/music           | explainer/部分管道质量下降 | preflight 声明；管道过滤；催服务端                                             |
| Remotion 打包           | 安装包 +100MB级 Node   | 可选组件/首次下载；Phase1 只用 ffmpeg                                         |
| EP 上下文爆炸              | 成本与跑飞              | stage 结束后 compact；工具结果落盘只传路径；revision caps                         |
| Skills 假设多 provider   | 幻觉调用不存在工具          | overlay skill + registry unavailable 硬失败                           |
| 删 ViMax 过快导致 Canvas 断 | 创作模式回归             | 强制 Step A 先上提 backends                                             |
| 团队误走「再写一套硬编码管线」       | 回到 ViMax 死胡同       | Code review 门禁：新阶段只能加 YAML/skills/tools，禁止新 `pipelines/*.rs` 业务状态机 |


---



## 14. 工程门禁（PR 规范）

1. **不允许**新增 ViMax 风格 `Script2VideoPipeline` 式硬编码业务阶段。
2. 新能力优先：YAML tools 列表 → Tool 实现 →（可选）skill 补丁。
3. 每个新 Tool 必须：schema、event、cost、unavailable 行为测试。
4. 变更 upstream 资产（MD/YAML）时记录与 OpenMontage 版本/commit 的映射。
5. Canvas PR 与 Montage PR 解耦：backends 上提单独合入。

---



## 15. 建议执行工单切片（可直接建 issue）

1. `media-backends-extract` — 上提 + Canvas 改依赖
2. `montage-crate-skeleton` — assets 嵌入、loader、schema validate
3. `montage-checkpoint-events` — 项目布局 + checkpoint + events
4. `montage-tools-flowy-ffmpeg` — selectors + flowy + stitch
5. `montage-orchestrator-ep` — EP loop + preflight
6. `montage-api-hitl` — `/api/montage` + approvals + SSE
7. `montage-ui-workspace` — 新工作区 + home pipelines
8. `vimax-userpath-removal` — 路由/UI/i18n 去 ViMax
9. `montage-governance` — promise/risk/cost
10. `montage-remotion` — Node sidecar
11. `montage-canvas-bridge` — materialize/sync
12. `vimax-crate-delete` — 物理删除旧 crate
13. `montage-explainer-e2e` — animated-explainer 验收
14. `tv-show-remount` — （可选）发布链路

---



## 16. 附录：Pipeline 启用策略


| Pipeline              | stability  | Phase | 备注                   |
| --------------------- | ---------- | ----- | -------------------- |
| `framework-smoke`     | beta       | **1** | CI 合同                |
| `cinematic`           | production | **1** | ffmpeg 成片主路径         |
| `hybrid`              | production | 1/2   | 有用户素材时               |
| `animated-explainer`  | production | **2** | 需 Remotion (+TTS 最好) |
| `animation`           | production | 2/3   |                      |
| `screen-demo`         | production | 3     |                      |
| `avatar-spokesperson` | production | 3     | 依赖 avatar/TTS        |
| 其余 beta               | beta       | 3+    | 按业务优先级               |


---



## 17. 一句话执行方针

> **资产原样、合同对齐、编排产品化、媒体 Flowy 化、ViMax 归零、Canvas 旁路保留。**

本方案即后续开发的 SoT；若与 OpenMontage 上游行为冲突，**默认以上游 AGENT_GUIDE / pipeline YAML / artifact schema 为准**，仅在 Flowy 能力缺口处通过 overlay 显式降级。