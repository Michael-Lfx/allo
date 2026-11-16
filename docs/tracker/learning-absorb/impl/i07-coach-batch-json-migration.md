---
id: i07
title: "教练批全迁新基建 + 概念评审契约化（g4 全迁）"
type: task
status: closed
assignee: zhangyj
blocked_by: [i06]
part_of: ../map.md
---

# i07: 教练批全迁新基建 + 概念评审契约化（g4 全迁）

**What to build:** 教练批管线同迁结构化输出新基建（用户明示全迁，反推荐项）：教练批输出
走 schema 单源 + 条款化 validator（ng.* 编号，与 i01 模块同族）；容错 deserializer 退役
（minutes 字符串、单对象数组、tier 中英文、裸标题等全部移除）；redraft ≤4 稿循环替换为
整批重试 ≤3 带 ng.* 条款化违规报告；provider json_object 硬要求（fail loud 同 i01）。
AI 概念评审门保留（语义兜底，与契约门分离），其输出（verdict/problems）同样契约化。
教练批提示词**保持内联常量**（契约迁移 ≠ 提示词资产化，两者解耦）。刚落库的教练批全量
重验，`service/tests.rs` 既有用例为底。

**Blocked by:** [i06 教练上下文重排](i06-coach-context-rework.md)（先定上下文与判据，
再迁解析/重试管道，避免同文件两线打架；i01 新基建为传递前提）。

**Status:** ready-for-agent

- [x] ng.* schema 单源 + validator + 互钉测试
- [x] 容错 deserializer 全量退役
- [x] 重试对齐 ≤3 带条款化违规报告（替换 GROWTH_MAX_DRAFTS ≤4 稿）
- [x] 概念评审输出契约化（评审门语义保留、失败保守放行语义不变）
- [x] `cargo test -p nomifun-learning -p nomifun-ai-agent` 全绿

## Resolution

**速览**：全项落地，commit `67ffd83a5`（feat/learn0928）。

- **growth_contract.rs（新）**：生长批形状契约 `ng.*`（顶层/节点/铸名键白名单、
  tier 词表、批量 ≤7、标题 ≤80 与批内唯一、minutes 1–60、difficulty 1–5、批内
  铸名唯一）+ 评审输出契约（verdict 词表、problems ≤8 与 field 词表）+ 两份
  JSON Schema 由同一份常量装配（互钉测试 + 金样白名单测试 + 红绿样
  11 测）。`ContractViolation` 类型与 `format_violations` 复用 structure_contract
  （同族单源）。
- **容错退役**：`RawBatch/RawNode/RawMint` + `de_optional_minutes`/
  `de_optional_concepts`/`de_string_list`/`de_option_list` 全删；
  `parse_coach_reply` 严格化——形状违约返回带 ng.* 报告的
  UnprocessableEntity（旧测试改为拒绝断言 + 新增合规正向测试含 trim 归一）。
- **语义门保留**（DB 事实，ng.* 编号）：登记表撞名 `ng.mints.collision`、
  引用可解析 `ng.refs.resolvable`、就绪即发布 `ng.assumes.uncovered`、课程内
  标题唯一 `ng.title.duplicate`。
- **重试对齐**：`GROWTH_MAX_DRAFTS=4` → `GROWTH_MAX_ATTEMPTS=3`；循环重构为
  统一轮次——形状违约/传输错误也带报告入轮次（首轮无稿则带反馈重新起草；
  门/评审打回保留「上一稿 + 报告」点修重裁），耗尽才失败（消息含最后一轮
  逐条款报告）。
- **json 接缝**：教练起草/重裁与概念评审全部改走 `complete_json_coach`
  （json_object + thinking ON + 480s 超时），预算按阶段自报（教练 32k、评审
  16k、结构 16k）；评审不可用（调用失败或本节违约）保守放行语义不变；
  `complete_coach` 文本包装退役（无消费者）。
- 验证：learning 181 测全绿（168+13）；ai-agent lib 1091 全绿；workspace
  check 干净。桥层无改动（接缝在 i01 已建，本轮只是消费者全量切换）。
