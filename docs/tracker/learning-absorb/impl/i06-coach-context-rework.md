---
id: i06
title: "教练上下文重排 + 既有门调整（g3 清单）"
type: task
status: open
assignee:
blocked_by: [i02]
part_of: ../map.md
---

# i06: 教练上下文重排 + 既有门调整（g3 清单）

**What to build:** `CoachContext::render` 重排为 9 块（顺序即注意力序）：学习目标（+
程度/基础声明）→ 结构参考材料（全量块摊开，learnhub `reference.rs` 行格式
`- 块名(slug)|主题一句|难度档|主干|无前置`/`|前置:A、B`，呈现序 = 难度档/前置链深/主干
序/块序，每批全量不裁剪）→ 前置地形门口（首批重点呈现，其后软参照）→ 当前就绪 → 花名
册 → 已教概念账 → 覆盖读数（改标「纯展示」）→ 行为摘要 → 概念登记表切片。范围参考
（learning_scope）被结构材料取代，`analyze_scope` / `SCOPE_SYSTEM` 随之退役。深度水位门
退役：删 `validate_batch_depth` + `FORMAL_TITLE_MARKERS`，反虚标职能移 AI 概念评审门
（扩「难度诚实与阶段/档位相符，可附拆分建议」判据），`difficulty` 字段保留（070 列不动）。
首级判据 learnhub 式重写：结构材料摊开 + 地形门口参考 + 凝练纪律（动作句/单一行为单元/
「要么本节点教它要么是常识，简单≠常识」）+ 空账本 ⇒ assumes 必空机械地板提示。

**Blocked by:** [i02 结构生成管线](i02-structure-generation-pipeline.md)（结构材料要有
真快照可摊）。连带保留项（无需另裁）：就绪即发布、R 软闸、终点条款已随 i01 删。

**Status:** ready-for-agent

- [ ] 9 块新构成 + 全量块摊开行格式与呈现序
- [ ] validate_batch_depth + FORMAL_TITLE_MARKERS 删除，评审门扩难度诚实判据
- [ ] 首级判据重写（含空账本地板提示）
- [ ] analyze_scope / SCOPE_SYSTEM 退役，learning_scope 不再写读
- [ ] `cargo test -p nomifun-learning` 全绿（service/tests.rs 既有用例为底）
