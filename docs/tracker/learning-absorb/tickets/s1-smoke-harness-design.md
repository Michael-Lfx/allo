---
id: s1
title: "真机冒烟入口设计（参考 learnhub smoke bins）"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: map.md
---

## Question

真机冒烟测试入口在 allo 怎么设计：哪些生成管线配冒烟 bin、接什么线（数据库、LLM key、
观测存档面）、每条管线的机械判据是什么？用户要求（2026-10-12）：本 effort 的实施必须
支持直接进行真机冒烟测试。

### learnhub 参照（已调研，`src-tauri/src/bin/`）

| bin | 职责 | 关键形态 |
|---|---|---|
| `smoke-run.rs` | 分解立课 → 生长首批×N，无头直跑 | 与应用同接线（key 解析 keyring→环境变量、模型读 settings、LLM 档案落观测面）；CLI 参数（--goal/--proficiency/--batch/--batches/--space）；逐阶段打印产物；**机械判据自检**（首批 assumes 全空，不过 exit 2） |
| `smoke-json.rs` | json_object 能力读数 | 合法 JSON 率、空内容/截断怪癖分类、KaTeX 转义保真；**读数面非门禁**（唯一硬判据 = 全灭 0/n）；探测调用直调 executor 不混入管线档案 |
| `smoke-loop.rs` | 学习闭环端到端合龙 | 分解 → 生长首批 → 教练循环×2（真 LLM 内容 + **确定性学习者替身**作答）→ 逐项自检；`--space` 缺省**新建干净临时目录**（从零合龙纪律）；EXIT=0 判据 |

### allo 侧待定差异

1. **bin 放哪**：allo 是 axum + SQLite 多租户服务（非 Tauri 单机）；冒烟 bin 落
   `nomifun-learning` 的 `src/bin/` 还是 `nomifun-app`（后者持有完整接线）？
2. **数据库**：learnhub 是文件空间（`--space` 可指临时目录）；allo 冒烟对什么库跑——
   每次新建临时 SQLite（从零合龙，对齐 smoke-loop 纪律）还是开发库？
3. **LLM key / provider**：nomifun-ai-agent 的 provider 配置在冒烟语境怎么解析（开发
   配置文件？环境变量？）。
4. **观测存档面**：learnhub 的 `ArchivingExecutor`（每次调用存 prompt/schema/response）
   在服务端的对应物——地图雾区「LLM 调用观测存档」条目由本票一并设计收口。
5. **机械判据**（对齐新契约）：structure/地形两产物全过契约门 + 门口对账；首批 assumes
   全空（空登记表机械地板）；生长批全过结构门 + 铸名不撞；json_object 读数面。
6. **覆盖范围**：结构生成（g2 双调用）、生长批、端到端合龙三档是否都配；可视化投影
   无 LLM 不需冒烟。

本票结案产出：一份冒烟入口设计（bin 清单、接线、判据、运行命令），进实施规格；实施本身
在规格落地时进行。

## Resolution

**速览**：六项设计定案——单 bin 多子命令挂 nomicore CLI，临时库从零合龙，凭据复用现链，
新 ArchivingCompleter 收口观测存档，四档判据对应四子命令。

1. **bin 放哪**：`nomicore`（`nomifun-app/src/main.rs`，clap `Command` 枚举已有 Doctor
   先例）新增 `learning-smoke` 子命令，实现模块住 `nomifun-app/src/learning_smoke.rs`。
   复用组合根接线模式（`services.rs`：DB pool → `LearningService::new` +
   `set_generation_dependencies`）。不落 `nomifun-learning/src/bin`——那里拿不到 ai-agent
   桥与迁移引导。
2. **数据库**：缺省**从零临时 SQLite**（跑 `nomifun-db` 迁移到临时文件，对齐 learnhub
   smoke-loop「新建干净临时目录」纪律）；`--db <path>` 可指现库（开发库冒烟）。
3. **provider / key**：`--provider <id> --model <name>` 参数（缺省取库内默认 provider）；
   凭据**复用 `resolve_provider_config` 现链**，不建第二套解析。数据目录沿用 nomicore
   现有全局参数。
4. **观测存档面**（收口原雾区「LLM 调用观测存档」）：nomifun-ai-agent 内新增
   **`ArchivingCompleter`**——包住任意 `LearningCompleter`，每次调用（成功/失败）落
   prompt/response/耗时到 `<data_dir>/observation/learning-smoke/<run-ts>/`（JSONL）。
   冒烟运行必挂；生产运行不挂（零开销）。探测型调用（json 读数）直调内层 completer，
   不混入管线档案（learnhub smoke-json 纪律）。
5. **机械判据**（对齐新契约，失败 = 非零退出）：
   - `structure`：双产物全过 gd.* 契约门 + 门口对账通过 + `structure_json` 落列；
   - `grow`：首批 assumes 全空（空登记表机械地板，learnhub smoke-run 同款 exit 2 纪律）；
     后续批全过结构门（就绪即发布）+ 铸名不撞；
   - `json`：json_object 读数面（合法 JSON 率/空内容/截断怪癖分类），**读数面非门禁**，
     唯一硬判据 = 全灭 0/n；
   - `loop`：端到端合龙——结构 → 首批 → 教练循环 ×N（**确定性学习者替身**：程序化
     完成/跳过节点驱动再生长），EXIT=0 判据。
6. **覆盖范围与运行形态**：四子命令 `structure` / `grow` / `json` / `loop`，全部无头、
   stdout 逐阶段打印、`--goal` 等参数对齐 learnhub。运行示例：
   `cargo run -p nomifun-app --bin nomicore -- learning-smoke loop --goal "…"`。
   bins 与管线同期实施（管线不存在时冒烟无对象）；`loop` 依赖学习节点完成/跳过的
   程序化入口。

