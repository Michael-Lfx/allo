---
id: i03
title: "读侧投影 API：GET /courses/{id}/graph/structure"
type: task
status: open
assignee:
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

- [ ] structure_view.rs 纯函数 + 单元测试（零 IO 零 LLM 零时钟）
- [ ] 路由 + 软删 404 守卫（对齐 course.rs 软删惯例）
- [ ] DTO：Rust 视图类型组 + TS 镜像 + learningApi 方法
- [ ] 投影不触碰概念登记表（测试断言）
- [ ] `cargo test -p nomifun-learning` 全绿 + `bun run typecheck`
