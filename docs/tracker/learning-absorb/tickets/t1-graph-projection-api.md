---
id: t1
title: "图投影只读 API：运行时重算、永不落盘"
type: task
status: closed
assignee: zhangyj
blocked_by: [g2]
part_of: map.md
---

## Question

为可视化提供只读投影接口（ADR-0002 铁律：投影运行时重算、永不落盘、不建关系表存边）。
learnhub 对应物是纯函数 `build_structured_view`（零 IO 零 LLM，`StructuredView{course,
structure, terrain}`）。

需要定的实事（非决策，g2 契约定后照单施工）：

1. 路由与 DTO：新只读端点（如 `GET /api/learning/courses/:id/structure-view`）；
   DTO 进 `nomifun-api-types`；对齐 allo 现有 WS 事件刷新模型（growth_* 之外新增
   structure_generated / structure_failed 事件）。
2. 投影内容（**v1 只做结构视图**，p1 已裁）：块视图预计算（名集/主干分组/矩阵格/依赖分层
   L0/`no_prereq` 布尔）+ 地形门口列表；learnhub 的读侧派生「零 LLM、零 IO、零时钟」原则
   照搬。**概念网边视图移出 v1**（概念表 Tab 已覆盖跨课程概念面，图形化留后续）。
3. **课程边界（用户 2026-10-12 铁律）**：投影严格课程内作用域——只读本课
   `structure_json` 快照重算，**不 join 概念登记表、不做块→概念任何解析或名字匹配**；
   跨课程概念事实不进本接口。
4. 缓存策略：多用户服务端要不要缓存层（learnhub 单机不需要；SQLite 读 + 纯函数重算可能
   已够快，先不缓存？）。
5. 权限与软删：对齐 069 软删语义（已删课程投影行为）。

完成后本票关闭，答案记录实际路由/DTO/函数位置，供前端结构图 Tab 实施时引用。

## Resolution

**速览**：五项工程实事按仓库既有惯例定案（design facts，实施照单施工）——

1. **路由**：`GET /api/learning/courses/{id}/graph/structure`（现有 `{id}/graph/*` 命名
   空间，`routes.rs` 加一行；handler `get_structure_view`；标准鉴权无新权限类）。课程
   不存在或已软删（service 层 `deleted_at IS NULL` 守卫，对齐 `course.rs:202` 惯例）→ 404。
2. **DTO**：沿学习模块现行惯例——Rust 侧 `nomifun-learning/src/models.rs` 新增视图类型组
   （serde），TS 镜像 `ui/src/renderer/pages/learning/types.ts` + `learningApi` 客户端方法。
   （注：AGENTS.md 的「HTTP DTO 归 nomifun-api-types」在该模块是既有偏离，随大流；把
   学习 DTO 迁 api-types 属独立重构，不在本 effort。）形状：
   `LearningStructureView { course_id, structure: Option<StructureViewDto>,
   terrain: Option<TerrainViewDto>, generated_at: Option<i64> }`——**200 恒答、None =
   未生成**（词汇表 Missing：合法空状态，不走错误路径；生成中状态由 WS 事件表达）。
   StructureViewDto 预计算：主干分组、块名集、主干×阶段矩阵格、依赖分层（L0 = `no_prereq`
   布尔）、枢纽出入度。
3. **投影函数**：`nomifun-learning/src/learning_graph/structure_view.rs` 新模块，纯函数
   `build_structure_view(&StructureDoc, Option<&TerrainDoc>) -> LearningStructureView`——
   零 LLM、零时钟，IO 只在 service 边界读一次快照行（learnhub `build_structured_view`
   同构）；单元测试随模块。`learning_graph/mod.rs` 已超 2000 行，不再内联。
4. **WS 事件**：`emit_graph_event` 族新增 `structure_started` / `structure_completed` /
   `structure_failed`（对齐 growth_* 命名）；`compass_updated` 随罗盘退役删除。前端结构图
   Tab 以事件触发重取投影。
5. **缓存**：v1 不做——一次行读 + 纯函数重算，profiling 有据再加。

**边界执行**（用户 2026-10-12 铁律）：投影严格课程内作用域——service 只读本课
`structure_json` 快照重算，**不 join 概念登记表、不做块→概念任何解析或名字匹配**；
跨课程概念事实继续走既有 concepts 端点与概念表 Tab。

