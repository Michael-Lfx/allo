---
id: i07
title: "教练批全迁新基建 + 概念评审契约化（g4 全迁）"
type: task
status: open
assignee:
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

- [ ] ng.* schema 单源 + validator + 互钉测试
- [ ] 容错 deserializer 全量退役
- [ ] 重试对齐 ≤3 带条款化违规报告（替换 GROWTH_MAX_DRAFTS ≤4 稿）
- [ ] 概念评审输出契约化（评审门语义保留、失败保守放行语义不变）
- [ ] `cargo test -p nomifun-learning -p nomifun-ai-agent` 全绿
