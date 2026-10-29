# Session 动态发现与使用 Skills

> **最后维护：** 2026-10-16  
> 范围：Nomi / ACP 会话（`nomifun-conversation` + `nomi-agent`）。不含 Vimax。  
> 产品默认：**允许模型按 L1 描述自主发现并调用**（`model-auto`）。

## 易懂正文

以前有两条几乎不通的轨：人用 `/` 选 skill，引擎只看见磁盘上的一小撮 skill。现在 Flowy catalog 在会话启动时并进引擎 L1，模型随时可以调常驻 `Skill` 工具。人点的 chip 仍是强制入口，只是不再禁止模型去加载没被点到的 catalog skill。

你可以把它想成目录卡片（短描述）和说明书（SKILL.md）分开：卡片一直在；说明书只在你点名或模型认为对口时才打开。脚本类 skill 打开前要过现有执行确认，不另做弹窗。某一条不想让模型自己用，就在 Skills Hub 关掉自动调用，或在 frontmatter 写 slash-only。会话里自动加载会出 chip，可以钉住，钉住后后面每轮都 force。

catalog 很大时，宿主先按词法给当前这句话排个序。默认排序结果可以进 L1；若要先观察命中率，设 `NOMI_SKILL_RANKING_SHADOW_ONLY=1`，只记 SQLite。超过大约 200 条才出现 `SkillSearch`。不要上 embedding，也不要在工具循环中途改工具表。

### 小样（10 个）

默认 `model-auto`：`officecli`、`officecli-docx`、`officecli-xlsx`、`officecli-pptx`、`news-briefing`、`planning-with-files`、`ui-ux-pro-max`、`star-office-helper`。  
脚本确认：`art-motion`（有 `scripts/`）。  
禁止隐式调用：`nomifun-webui-setup`（`disable-model-invocation: true`）。

未选 skill 的 Nomi 回合：system 里应出现 catalog 短列表和 `Skill` 工具；Session Logs 里不应再只有 auto-inject。自动加载成功后，消息流出现 `skill_load`（Auto 徽章），并可钉住。

---

## 底稿

### 不变量

1. 发现层只来自已授权 catalog。
2. L1 只放元数据；正文只在 force 或 Skill 工具成功后进入，并记 `skill_load`。
3. `Skill` 工具常驻。宿主只在每条用户消息重算 topK，不在每个 tool 结果后改 system。

### 关键落点

| 层 | 位置 |
| --- | --- |
| Catalog → 引擎 | `nomifun-ai-agent/catalog_bridge.rs`，`factory/nomi.rs` `host_catalog_skills`，`nomi-skills/loader.rs` `merge_host_catalog_skills` |
| L1 列表 | `nomi-skills/prompt.rs`（2% 预算 + canonical id），`prompt_graph.rs` `skills_for_profile` 过滤 slash-only |
| Skill 工具 | `nomi-agent/skill_tool.rs`：canonical id、slash-only 拒绝、脚本 `category_for=Exec` |
| SkillSearch | `skill_search_tool.rs`，阈值 `SKILL_SEARCH_CATALOG_THRESHOLD=200` |
| 每轮排序 | `ConversationService::prepare_catalog_discovery`，表 `skill_selection_event` |
| Force / 钉住 | `inject_skills` 合并 `conversation_skill_pins`；`inject_loaded_skill_context` |
| 自动账本 | `stream_relay.rs` `persist_auto_skill_load` |
| ACP 首条 | heavy mode 合并 `catalog_index`（cap 40）；light mode 仍交给原生 CLI |
| 策略覆盖 | `skill_invocation_overrides`，`PUT /api/skills/invocation` |
| UI | `SessionSkillModeBar`、`MessageSkillLoad` Auto/Pin、Skills Hub Switch |

### `extra_skill_dirs`

引擎仍接受额外扫描目录，但 Flowy 会话走 **元数据合并**（带正文的 `SkillMetadata`），不再靠会话侧写入 `extra_skill_dirs`。这样 canonical id 与 invocation_policy 不会在二次扫描时丢掉。

### 环境变量

- `NOMI_SKILL_RANKING_SHADOW_ONLY=1`：超预算时词法结果只入库，不改 L1。

### 非目标

改 Vimax；用 `/` 选择逻辑猜自动路由；动态增删工具 schema；把全量 SKILL.md 灌进 system prompt；本阶段上 embedding。
