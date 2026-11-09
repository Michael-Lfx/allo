---
id: c7
title: "概念后端端点：单概念详情 + 全局总览 + other_courses 带 course_id + Rust 测试"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# c7: 概念后端端点（nomifun-learning）

**What to build:** 两个全局（不挂课程）只读端点 + 一处 DTO 形状升级：

1. `GET /api/learning/concepts/{concept_id}` → `ConceptDetailView`：
   - 条目：concept_id / canonical / definition / aliases（复用
     `GraphConceptAliasView`）/ source / deprecated / created_at / updated_at；
   - 关系（纯 registry 读时推导，零 LLM、不落库，与 CONTEXT「关系读时推导」
     一致）：`self_reference`、`mutual`（互指，全部 partner）、`mentions`
     （本概念定义提到的其他概念）、`mentioned_by`（定义提到本概念的其他概
     念），各项 `{concept_id, canonical}`；
   - 使用：按课程分组的节点引用 `usages: [{course_id, course_title, refs:
     [GraphConceptRefView…]}]`（teaches + assumes 全量，`deleted_at IS NULL`）；
   - 声明：`declared_in: [{course_id, title}]`（`learning_basis_declarations.
     concept_ids_json` 反查）。
2. `GET /api/learning/concepts` → `Vec<ConceptOverviewRowView>`：非 deprecated
   registry 全量（沿用 500 上限），每行 concept_id / canonical / definition /
   alias_count / course_count / ref_count / declared_course_count /
   self_reference / mutual_count。
3. `GraphConceptRowView.other_courses`：`Vec<String>` → `Vec<{course_id,
   title}>`（前端 types.ts 与 ConceptTable 渲染同票适配，保证每个提交运行时
   不断）。

## 法证证据（2026-11-09 实码核实）

- 无单概念 HTTP 端点：`search_concepts` detail 模式 + `registry_entry_by_id`
  （learning_graph.rs:4735-4750 / 4953-5003）只暴露给 AI 工具
  （`nomifun-ai-agent/src/lesson_content_loop.rs:1817`）。
- 无全局列表：`registry_entries_bounded`（:4705-4733，上限 500）仅内部用；
  registry 全局表无 course/user 列（070 :182-201），「全部概念」= 全 registry。
- 节点链接：`learning_lesson_concepts`（070 :203-225）concept_id 有索引
  （:254-259），按概念查使用面廉价；`graph_concepts` 已有同形 join
  （learning_graph.rs:4256-4260）可改编。
- 关系：`definition_audit.rs` `self_references`（:176-185）与
  `mutual_cycles`（:187-210）是 canonical 名纯子串扫描——不依赖课程词表，
  可全局复用；单向提及是同一扫描的非对称一半。
- other_courses 只给标题：learning_graph.rs:4281-4297（teaches-only、无 id）。
- 路由面：`routes.rs` 现有概念路由 :67-74（`graph_concepts` 404-413 /
  `concept_audit` 458-470 handler 形状可照抄）。

## 实施要点

- [ ] models.rs：`ConceptDetailView` / `ConceptOverviewRowView` /
      `ConceptLinkView` / `ConceptCourseUsageView` / `ConceptCourseLinkView`
      （serde 形状与现有 View 一致）；`GraphConceptRowView.other_courses`
      换类型。
- [ ] service：`concept_detail(concept_id)`（未知 id → 404，deprecated 照常
      返回并带 `deprecated: true`）、`concepts_overview()`；关系扫描复用
      definition_audit 的匹配原语（不复制逻辑）。
- [ ] routes.rs 注册两条路由（与既有 learning 路由同一鉴权面）。
- [ ] `graph_concepts` 的 other_courses 查询带出 course_id（语义不变：
      teaches-only、排除本课）。
- [ ] 前端最小适配：types.ts 新 DTO + other_courses 新形状；ConceptTable 渲染
      `.title`（链接化留 c8）；api.ts 加 `conceptDetail` / `conceptsOverview`
      client。
- [ ] Rust 测试（新文件，注册到 service/mod.rs，fixture 照 basis_tests.rs）：
      ①详情——多 partner 互指全返回、单向提及双向正确、跨两门课 refs 按课程
      分组、声明反查命中；②未知 id 404；③总览——deprecated 排除、计数正确；
      ④other_courses 带 id 且排除本课。

## 验收

- `cargo test -p nomifun-learning` 全绿（新测试 + basis/graph 既有测试零回归）；
  `cargo fmt -p nomifun-learning`；`cargo check --workspace`。
- `bun run typecheck` 绿（前端适配）。

## 非目标

- 总览分页/检索（后置清单）；缺口片段（依赖课程词表，留在课程内审计）；概念
  编辑/合并写操作；前端页面（c8）。

## Resolution（2026-11-09 施工收口）

**全验收过，闭票。** 施工 = `7230cb291`（8 文件；nomifun-learning +480/−7）。

- **两个全局端点**（routes.rs，挂在课程路由之外——registry 无课程归属列）：
  `GET /api/learning/concepts`（`concepts_overview`）与
  `GET /api/learning/concepts/{concept_id}`（`concept_detail`，未知 id =
  `AppError::NotFound`）。`/concepts` 与 `/courses` 前缀不同，无路由遮蔽。
- **关系 = 读时推导的纯函数** `concept_relation_index`（零 LLM 零写库）：自指/
  互指直接复用 `definition_audit::self_references` / `mutual_cycles`（与课程内
  审计同口径，不复制逻辑）；单向提及 = 同一子串原语的非对称一半。**展示契约
  （票内定）**：互指 partner 从单向 `mentions/mentioned_by` 剔除——一条关系只
  出现一处（测试钉死；首轮测试预期按「两处都出现」写错，以契约为准订正）。
- **使用面**：teaches + assumes 全量、未删课程、按课程分组（查询序稳定）、带学
  习者进度；**声明面**：锚行 `concept_ids_json` 反查，一课多次声明去重。
- **总览**：非废弃 + `SEARCH_REGISTRY_LIMIT` 同界（命中 warn）；别名/课程/引用/
  声明课程计数 + 自指/互指旗标；缺口片段依赖课程词表，留在课程内审计（非目标）。
- **DTO 升级**：`GraphConceptRowView.other_courses` → `Vec<ConceptCourseTitleView>`
  （SQL 带出 course_id，teaches-only 与排除本课语义不变）；后端/脚本/web 无其
  它消费方（grep 实证）。别名解码抽 `alias_views` 供详情/总览复用。
- **前端最小适配**（同提交，运行时不断）：types.ts 新 DTO + 新形状；api.ts
  `conceptsOverview` / `conceptDetail`；概念表 `other_courses` 渲染 `.title`
  （链接化留 c8）。
- **测试**：新 `concept_view_tests.rs` 3 例（详情关系/使用分组/声明反查/自指；
  未知 id 404 + 总览计数与 deprecated 排除；other_courses 带 id 且排除本课）；
  **nomifun-learning 全量 944 passed / 0 failed**；`cargo fmt -p` 只动本票文件；
  typecheck 55 错持平存量（learning 零错）；`check:error-surface-contract` /
  `check:support-surface-contract` 绿。
- **调试记档**：首轮红 = 声明反查 SELECT 漏列 `concept_ids_json`（运行时 "no
  column found"，已修）；第二轮红 = 测试预期与互指契约冲突（已订正测试）。临时
  探针测试已删，零残留。
