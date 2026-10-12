---
id: i02
title: "结构生成管线：提示词资产 + 双调用契约门 + 快照落库"
type: task
status: open
assignee:
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

- [ ] 建课 API 收三项输入（程度/基础可选，缺省 LLM 扩写程度、纯零基础）；不再收终点语义
- [ ] 双调用 + 门口对账 + 整批重试 ≤3 带 gd.* 逐条款报告；耗尽 = 建课失败明示，无半成品
- [ ] structure_json 快照原子落库（单 UPDATE），重跑覆盖
- [ ] provider 不支持 json_object → 建课即失败并明示原因
- [ ] 提示词资产 + 互锁/围栏/金样测试三件套
- [ ] 入口域盘点退役
- [ ] WS structure_* 三事件
- [ ] ArchivingCompleter + learning-smoke structure/json 子命令
- [ ] `cargo test -p nomifun-learning -p nomifun-app` 全绿
