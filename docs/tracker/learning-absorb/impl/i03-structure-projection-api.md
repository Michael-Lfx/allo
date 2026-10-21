---
id: i03
title: "读侧投影 API：GET /courses/{id}/graph/structure"
type: task
status: closed
assignee: zhangyj
blocked_by: [i01]
part_of: ../map.md
---

# i03: 读侧投影 API：GET /courses/{id}/graph/structure

**What to build:** 只读结构投影接口（t1 design facts 照单施工）：新路由
`GET /api/learning/courses/{id}/graph/structure`，handler `get_structure_view`；纯函数
`build_structure_view(&StructureDoc, Option<&TerrainDoc>)` 住 `learning_graph/
structure_view.rs` 新模块（零 LLM、零时钟，IO 只在 service 边界读一次快照行）；DTO 沿
学习模块惯例住 `models.rs` + TS 镜像 `ui/.../learning/types.ts` + `learningApi` 客户端
方法。200 恒答、`structure: null` = 未生成（Missing 合法空状态）；课程不存在或已软删 →
404。预计算：主干分组、块名集、主干×阶段矩阵格、依赖分层（L0 = no_prereq 布尔）、枢纽
出入度。**课程边界铁律：只读本课 structure_json 重算，不 join 概念登记表、不做块→概念
任何解析或名字匹配。** v1 无缓存。

**Blocked by:** [i01 地基](i01-foundation-migration-contract-infra.md)（需
structure_json 列；未生成时返回空视图即可验证）。

**Status:** ready-for-agent

- [x] structure_view.rs 纯函数 + 单元测试（零 IO 零 LLM 零时钟）
- [x] 路由 + 软删 404 守卫（对齐 course.rs 软删惯例）
- [x] DTO：Rust 视图类型组 + TS 镜像 + learningApi 方法
- [x] 投影不触碰概念登记表（测试断言）
- [x] `cargo test -p nomifun-learning` 全绿 + `bun run typecheck`

## Resolution

**速览**：全项落地，commit `0b59fa16f`（feat/learn0928）。

- `learning_graph::structure_view`（新模块，公开）：`build_structure_view(course_id,
  Option<&Value>) -> LearningStructureView` 纯函数。预计算面全按 t1：主干分组
  （块表序）、块名集（升序）、主干×阶段矩阵、依赖分层（**Kahn 拓扑**——L0 =
  不做任何 assumes 边之源的块；环上块冻结在可达深度而不是挂死，单测覆盖）、
  枢纽 Top10（assumes + composes 出入度和降序，并列名序）。
- 路由 `GET /api/learning/courses/{id}/graph/structure`：200 恒答（structure/
  terrain 均 null = 未生成）；软删或不存在 → 404（`deleted_at IS NULL` 一查定
  生死，无双查）。`POST .../structure/regenerate` 已在 i02 落位。
- DTO 组住 `models.rs`（Scalars/Domain/Spine/Block/Edge/Gap/Quality/Group/
  Matrix/Hub/Door 等 13 型），TS 镜像 `ui/.../learning/types.ts` + `learningApi
  .graphStructure/.regenerateGraphStructure`。
- 边界执行：service 一次行读 + 纯函数重算；无任何登记表查询（测试以空登记表
  建影验证）；v1 无缓存。
- 验证：learning 169 测全绿（structure 相关 35：契约 18 + 提示词 6 + 门面 4 +
  投影 6 + 服务级 1）；`bun run typecheck` 学习页零错误（余 54 个全在
  videoCanvas/videoGeneration，origin 存量基线，与本次无关）。
- 顺带清理：i01 遗留的死代码 `require_graph_course`（无调用者，已被
  `require_live_graph_course` 取代）与一个测试未用变量告警一并删除。
