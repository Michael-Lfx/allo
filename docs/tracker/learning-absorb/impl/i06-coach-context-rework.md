---
id: i06
title: "教练上下文重排 + 既有门调整（g3 清单）"
type: task
status: closed
assignee: zhangyj
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

- [x] 9 块新构成 + 全量块摊开行格式与呈现序
- [x] validate_batch_depth + FORMAL_TITLE_MARKERS 删除，评审门扩难度诚实判据
- [x] 首级判据重写（含空账本地板提示）
- [x] analyze_scope / SCOPE_SYSTEM 退役，learning_scope 不再写读
- [x] `cargo test -p nomifun-learning` 全绿（service/tests.rs 既有用例为底）

## Resolution

**速览**：全项落地，commit `3f155b9d8`（feat/learn0928）。

- **9 块重排**（顺序即注意力序，`CoachContext::render` 头注写明）：学习目标
  （+程度/基础声明附注，来自结构/地形产物的标量回执）→ 结构参考材料（全量块
  摊开，行格式 `- 块名(slug)|主题一句|难度档|主干|无前置`/`|前置:A、B`，呈现序
  = 难度档/前置链深/主干序/块序，新纯函数 `structure_view::render_reference_lines`）
  → 地形门口（首批「首批入口重点参照」框架语 + 逐门落格纪律；其后「软参照」）
  → 当前就绪 → 花名册 → 已教概念账 → 覆盖读数（纯展示）→ 行为摘要 → 登记表
  切片。
- **首级判据重写**：6 条规则长文（含入口域盘点两态分支）删除，收缩为
  「首批纪律」三行——入门档起步 + 动作句/单一行为单元/「要么教它要么是常识，
  简单≠常识」+ 空账本 ⇒ assumes 必空机械地板；COACH_SYSTEM 的起点纪律首条
  改指【学习目标】的基础声明、删除深度水位纪律、自查第 7 条改「如实申报
  （形式化/证明类不得压档）」。
- **水位门退役**：`validate_batch_depth` + `FORMAL_TITLE_MARKERS` + 专属测试
  删除；`gate_batch` 去掉 applied_batches 查询；`ProposedNode.difficulty` 保留
  （呈现序/花名册仍消费，070 列不动）。评审门扩第 4 条判据「难度诚实与阶段/
  档位相符」（与结构参考材料同名块档位对照、可附拆分建议），problems 的
  field 词表加 `difficulty`。
- **analyze_scope 退役（实施偏差）**：核查发现裁决前提漏列消费方——传统课程
  agent 大纲管线（`course_outline`，scope blocks 覆盖率账）仍在调用它。按
  「不回归既有功能」处理：`SCOPE_SYSTEM`/`ScopeAnalysis`/解析/调用整体原样
  迁移到新模块 `course_outline/scope.rs`（含独立 de_string_list），token 常量
  更名 `OUTLINE_SCOPE_MAX_TOKENS`；**学习图侧**彻底不写读 learning_scope
  （建课 learning_goal=原始目标、不写 scope 列；`LearningGraphView.scope` 字段
  与 TS 镜像删除；教练上下文不再读该列）。
- **连带迁移**：图课程课时内容生成的 grounding 从 `learning_scope` 文本改为
  快照的 `proficiency_statement`（`GraphLessonContext.scope` → `degree_statement`，
  引擎 prompt「学习范围：」→「程度声明：」，断言同步）。
- 验证：learning 168 测全绿；workspace check 干净；学习页 typecheck 零错误；
  全量并发跑中 ai-agent 两个失败（factory/nomi、provider_health）隔离重跑
  皆过——既有并发 flaky，与本次无关。
