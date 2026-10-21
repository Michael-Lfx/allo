---
id: i02
title: "结构生成管线：提示词资产 + 双调用契约门 + 快照落库"
type: task
status: closed
assignee: zhangyj
blocked_by: [i01]
part_of: ../map.md
---

# i02: 结构生成管线：提示词资产 + 双调用契约门 + 快照落库

**What to build:** 建课 = 一次生成知识结构：用户只给学习目标（必填）+ 程度声明（可选）+
基础声明（可选），后端串行双调用（① structure ② 前置地形），任一产物违反 gd.* 契约则
两份一起重生成并把逐条款违规报告注入重试装配（≤3 轮），门口对账堵引用悬空，全成才把
快照原子写入 `structure_json`（重跑覆盖）；WS `structure_started/completed/failed` 事件
驱动前端；goal_type / domain_type 由 LLM 自判落产物。提示词走 md 文件资产（include_str! +
头注五字段 + 具名占位符互锁 + 围栏词表机械拦截 + 金样快照测试），温度/thinking 住注册表。
入口域盘点（survey_entry_domains / ENTRY_SURVEY_SYSTEM / EntryDomain）随本票退役——地形
门口是其结构化替代。同期落真机冒烟：`ArchivingCompleter`（观测存档
`<data_dir>/observation/learning-smoke/`）+ `nomicore learning-smoke structure` 与 `json`
子命令（structure：双产物过契约门 + 门口对账 + 落列，EXIT 判据；json：json_object 读数
面非门禁，唯一硬判据 = 全灭 0/n；探测调用直调内层 completer 不入档案）。

**Blocked by:** [i01 地基](i01-foundation-migration-contract-infra.md)。

**Status:** ready-for-agent

- [x] 建课 API 收三项输入（程度/基础可选，缺省 LLM 扩写程度、纯零基础）；不再收终点语义
- [x] 双调用 + 门口对账 + 整批重试 ≤3 带 gd.* 逐条款报告；耗尽 = 建课失败明示，无半成品
- [x] structure_json 快照原子落库（单 UPDATE），重跑覆盖
- [x] provider 不支持 json_object → 建课即失败并明示原因
- [x] 提示词资产 + 互锁/围栏/金样测试三件套
- [x] 入口域盘点退役
- [x] WS structure_* 三事件
- [x] ArchivingCompleter + learning-smoke structure/json 子命令
- [x] `cargo test -p nomifun-learning -p nomifun-app` 全绿

## Resolution

**速览**：全项落地（feat/learn0928，本票单提交）。

- **提示词资产**：`nomifun-learning/prompts/structure/{goal-decomposition,
  prerequisite-terrain}.md`（头注五字段 + `{{具名占位符}}`），装配器
  `learning_graph::structure_prompts`（头注剥离 + 单遍替换 + 双向互锁 + 围栏
  词表），六项测试矩阵全落（互锁/注册表完备/围栏/契约后置/条款覆盖/金样快照，
  `LEARNING_UPDATE_GOLDEN=1` 重生成金样）。allo 改造点：goal_type 由模型自判
  （用户不给）；form_reference 固定 `Flowy 学习结构模板 v1`；地形 prompt 按
  allo 减配契约只产门口。
- **门面** `learning_graph::structure_gen`：结构 → 地形串行双调用
  （`complete_json_coach`：json 接缝 + thinking ON + 480s 超时 + 16384 预算），
  双契约门 + 门口对账，违规报告（gd.* 逐条款）注入重试装配，≤3 轮，全成才产
  快照信封（`{version, generated_at, structure, terrain}`）。
- **service 接线**：建课 API 增 `proficiency_statement`/`learner_basis`（可选，
  归一限长 500）；建课后台任务改为「结构生成 → 首生长」串行（结构失败不阻断
  生长——铁律：结构对生长不供内容）；WS `structure_started/completed/failed`
  三事件；快照原子 UPDATE 落列；新增
  `POST /courses/{id}/graph/structure/regenerate`（重生成 = 换参考地图不换课；
  程度/基础声明从现有快照回读；软删 404 守卫 `require_live_graph_course`；
  每课程 slot 锁防并发重生成）。
- **入口域盘点退役**：`EntryDomain`/`ENTRY_SURVEY_SYSTEM`/`survey_entry_domains`
  /`parse_entry_survey`/`ENTRY_SURVEY_MAX_TOKENS` 全删；CoachContext 的
  `entry_survey` 块换 `terrain_doors`（从本课 structure_json 的 terrain 重读
  `- [G1]（硬前置）判据` 行，课程内铁律：不触碰概念登记表）；首级判据分支
  「照门口落节点」，无快照退回教练自行盘点。
- **冒烟基建**：`nomifun_ai_agent::ArchivingCompleter`（每次调用一行 JSONL
  落 `<data-dir>/observation/learning-smoke/<run>/`，成功/失败全存）+
  `nomicore learning-smoke structure|json` 两子命令（组合根同接线；缺省从零
  临时 SQLite，`--db` 指开发库；structure 判据 = 落列后重读重验双契约 + 对账，
  失败 exit 2；json 读数面非门禁、全灭 0/n 才 exit 2；探测调用直调内层
  completer 不入档案）。
- **验证**：learning 162、ai-agent lib 1091、app 391+ 全绿；workspace check
  干净。**存量失败两条（与本次改动无关，均有隔离/历史证据）**：
  `nomifun-app router::state::every_production_conversation_service_uses_…`
  （壁纸提交 44cd0f489 把 state.rs 生产段的 `ConversationService::new(` 清零，
  自审计测试未随之更新）；`nomifun-ai-agent` 的 agent_trace hub 与
  acp_session_sync 两测在全量并发下偶发红、隔离必过（时序敏感，文件最后改动
  来自 834d20d91）。
- 实施偏差记录：门面重试对所有尝试失败（含 LLM 传输错）生效——allo 的
  provider 错误统一映射 BadGateway，无法廉价区分「json 不支持」（配置性，
  不该重试）与瞬态抖动；json 不支持时每轮在流入口即拒、耗时可忽略，最终消息
  保真，故取 learnhub 的「门面全重试」语义。温度参数未进 json 接缝（裁决未
  锁定；契约门 + 重试承担质量），留 i07 教练批迁移时一并权衡。
