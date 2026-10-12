---
id: t1
title: "图投影只读 API：运行时重算、永不落盘"
type: task
status: open
assignee:
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
