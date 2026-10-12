---
id: i01
title: "地基：迁移 071 + 结构契约文档 + 结构化输出新基建"
type: task
status: closed
assignee: zhangyj
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

- [x] 迁移 071：DROP `learning_course_endpoints` + 删终点零正文标记课时行 + DROP
      `learning_courses.compass_md` + ADD `learning_courses.structure_json`（TEXT NULL）
- [x] 存量失败批次不误判（09-29 教训）；066 破坏性迁移先例风格
- [x] 契约文档落 `docs/contracts/learning-structure-artifacts.md`：gd.* 条款化编号，
      structure 全套（8 标量 + domain + spines 6–12 + blocks 40–80 五键纪律 + edges 三型
      + gaps≥3 + quality≥5 + no_schedule）+ 前置地形（门口 G 判据）+ 门口对账条款 +
      存储条款（structure_json 单字段快照、重跑覆盖、投影永不落盘）+ 课程边界铁律
- [x] nomifun-learning 新契约模块：词表/键集常量单源、JSON Schema 单源（与 validator 同
      源）、gd.* validator（违规带条款编号）、门口对账函数、schema↔validator 互钉测试、
      逐条款红绿样测试
- [x] `LearningCompleter` 新增 json 模式方法（thinking 开/关两通道对齐现状）；
      `LiveLearningCompleter` 落 response_format=json_object，不支持即显式报错
- [x] 终点/罗盘后端读写面全量移除，`compass_updated` WS 事件删除
- [x] `cargo test -p nomifun-learning -p nomifun-db -p nomifun-ai-agent` 全绿；
      `cargo fmt`；`cargo check --workspace`

## Resolution

**速览**：全项落地，commit `269f67eb0`（feat/learn0928）。

- 迁移 071（破坏性，066 先例风格）：终点表 + 标记课时行 + 进度行清删、
  `compass_md`/`compass_updated_at` 列删除、`structure_json` 快照列新增（NULL =
  未生成，CHECK 只收 JSON 对象）；071 专属迁移测试覆盖全部断言（含进度行清理与
  CHECK 拒非 JSON）。
- 契约文档 `docs/contracts/learning-structure-artifacts.md` v1.0：gd.* 条款与
  validator 1:1（含 learnhub 机械读法放宽留痕——one_name 词边「和」豁免、
  no_schedule 排期形态词表 + quality/disclaimer 元声明面豁免）；另含快照信封、
  生成轮次、与 learnhub 的差异清单（每行带裁决依据，防"补齐"回旧形态）。
- 新基建：`learning_graph::structure_contract`（词表/量级档单源 + gd.*/gd.terrain.*
  validator + 门口对账 + JSON Schema 单源，18 项测试含互钉与逐条款红绿样）；
  `LlmRequest.json_object` 贯通 nomi-types → openai 兼容层 response_format，
  anthropic/vertex/bedrock/openai_responses fail-loud 拒绝；
  `LearningCompleter::complete_json(_with_thinking)` 默认实现 fail-loud，
  `LiveLearningCompleter` 实现两通道（BadGateway 瞬态重试语义沿用）。
- 终点/罗盘后端面移除超出票面列举的部分：`service/lesson.rs` 两处
  `learning_course_endpoints` 子查询（上下文组装/总节点数）、nomifun-ai-agent 的
  `GenerateCourseRequest` 组装点；`LearningEndpointId` 从 nomifun-common 退役。
  入口域盘点与 analyze_scope 按票面说明保留（分别随 i02/i06 走）。
- 验证：nomifun-learning 153、nomifun-db 458+、nomifun-ai-agent 1090+ 全绿；
  `cargo check --workspace` 干净；fmt 已跑。UI 未动（propose 404 走既有空列表
  降级；compass/endpoints 字段消失按空值渲染），i04 清场。
- 环境注记：施工中 C 盘打满（0 字节），`bun run clean` 释放 155.1G
  （build.noindex + target 增量缓存膨胀，prune-build 头注记载的已知病灶）。
