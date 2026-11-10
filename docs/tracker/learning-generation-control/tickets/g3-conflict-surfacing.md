---
id: g3
title: "生成任务冲突显性化——regenerate 静默吞掉收口"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# g3: 生成任务冲突显性化

**What to build:** `regenerate_structure` 的 registry 冲突改为同步暴露
（kicked=false+原因，或与 growth 同款 409）；前端 toast「结构生成已在
进行中」（zh-CN/en-US 双语对称，跑 `gen:i18n`+`check:i18n`）。顺带核查
（验收内）：lesson 任务冲突无静默路径；auto_refill 自动触发冲突不弹
用户错误框（静默降级，manual 才提示）。

**验收：**
- 任务运行中点 regenerate：UI 收到明确反馈，不再 200 kicked=true 静默
- 冲突路径有单测断言（现状 spawn 冲突仅 WARN 无断言）

**病历依据：** 2026-11-09 用户实点一次：1ms 200 + 后台 WARN + UI 零反馈。
对照 growth 409 → Modal.error（LearningGraphWorkspace.tsx:329）已显性。

**施工补充（探察 2026-11-10）：** ①`generate_course` 路径存在**同款静默**
（spawn 后 Conflict 仅 WARN），一并收口，不另立票。②lesson 冲突实为不静默
（pre_register 同步 409 + Message.error）——**只核不改**。③auto_refill 的
静默降级是正确行为——**勿动**。④施工陷阱：勿在路由层预拿 slot 锁（会与
run_structure_generation 内部拿锁二次冲突）；冲突链与单测模板见共享笔记 g3.md。
