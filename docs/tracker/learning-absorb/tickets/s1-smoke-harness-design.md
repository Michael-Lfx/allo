---
id: s1
title: "真机冒烟入口设计（参考 learnhub smoke bins）"
type: task
status: open
assignee:
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
