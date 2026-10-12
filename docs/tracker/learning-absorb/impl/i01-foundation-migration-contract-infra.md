---
id: i01
title: "地基：迁移 071 + 结构契约文档 + 结构化输出新基建"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# i01: 地基：迁移 071 + 结构契约文档 + 结构化输出新基建

**What to build:** 学习图课程的存储与契约地基一次就位——存量终点/罗盘数据破坏性退役、
`structure_json` 快照列可写；`docs/contracts/learning-structure-artifacts.md` 以 gd.* 条款
编号定义 structure 与前置地形两产物的机器契约；nomifun-learning 侧新增 schema 单源 +
gd.* validator（schema↔validator 互钉测试锁词表）；`LearningCompleter` 桥新增 json 接缝
（response_format=json_object，provider 不支持即报错 fail loud，不降级）。

**Blocked by:** None (can start immediately)。

**迁移破坏性说明**：删列/删表即破坏全部读写点，本票**必须随迁移移除后端终点/罗盘全部
读写面**（propose-endpoints 路由、regen_compass 管道、EndpointVerdict、停摆臂第二臂、
validate_batch 终点保护条款、教练上下文终点锚/罗盘块、模型字段与查询列、相关 DB 契约
测试）。入口域盘点（survey_entry_domains）与 analyze_scope 生成流消费**不在本票**——
它们不碰被删列，分别随 i02（地形替代）与 i06（范围参考退役）走。前端不动：propose 404
走既有空列表降级，create 请求容忍 endpoints 字段但忽略，i04 再清 UI。

**Status:** ready-for-agent

- [ ] 迁移 071：DROP `learning_course_endpoints` + 删终点零正文标记课时行 + DROP
      `learning_courses.compass_md` + ADD `learning_courses.structure_json`（TEXT NULL）
- [ ] 存量失败批次不误判（09-29 教训）；066 破坏性迁移先例风格
- [ ] 契约文档落 `docs/contracts/learning-structure-artifacts.md`：gd.* 条款化编号，
      structure 全套（8 标量 + domain + spines 6–12 + blocks 40–80 五键纪律 + edges 三型
      + gaps≥3 + quality≥5 + no_schedule）+ 前置地形（门口 G 判据）+ 门口对账条款 +
      存储条款（structure_json 单字段快照、重跑覆盖、投影永不落盘）+ 课程边界铁律
- [ ] nomifun-learning 新契约模块：词表/键集常量单源、JSON Schema 单源（与 validator 同
      源）、gd.* validator（违规带条款编号）、门口对账函数、schema↔validator 互钉测试、
      逐条款红绿样测试
- [ ] `LearningCompleter` 新增 json 模式方法（thinking 开/关两通道对齐现状）；
      `LiveLearningCompleter` 落 response_format=json_object，不支持即显式报错
- [ ] 终点/罗盘后端读写面全量移除，`compass_updated` WS 事件删除
- [ ] `cargo test -p nomifun-learning -p nomifun-db -p nomifun-ai-agent` 全绿；
      `cargo fmt`；`cargo check --workspace`
