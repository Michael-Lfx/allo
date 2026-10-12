---
id: g2
title: "结构生成契约与存储落点"
type: grilling
status: open
assignee:
blocked_by: [g1]
part_of: map.md
---

## Question

learnhub 的结构产物（8 标量键 + domain + spines 6–12 + blocks 40–80 + edges 三型 +
gaps + quality）与前置地形（门口 G1..Gn）引入 allo 时，契约与存储怎么落？

需要裁决：

1. **产物取舍**：structure 全套引入还是裁剪（例如 allo 已有 `learning_goal`/`learning_scope`
   与入口域盘点，与 goal_type/proficiency_statement/learner_basis、前置地形功能重叠）？
   前置地形要不要独立产物，还是并入 structure 一次生成？g1 已裁决罗盘退役、人读概览交给
   结构读侧投影——learnhub 同位产物是 knowledge-system 文档，要不要一并产出由本票定。
2. **SQLite 落点**：结构文档快照存哪个表哪个字段（单字段 JSON 快照语义；ADR-0002 铁律：
   投影永不落盘、不建关系表存块间边、运行时重算）；快照与课程行的关系（一课一快照？
   重跑 = 新快照版本？learnhub 是重跑同目标 = 新课并列，多用户服务端是否照搬）。
3. **生成轮次**：learnhub 每轮 2 次串行调用 + 门口对账 + 整批重试 ≤3 带反馈，照搬还是
   合并为单次调用（allo 的 completer 已有 thinking/非 thinking 双通道与 480s 预算先例）？
4. **契约文档**：是否照 learnhub 建 `docs/contracts/` 式条款化契约（gd.* 编号即稳定机器
   标识）放进 allo 的 `docs/architecture/learning` 域文档。

约束：建课交互形态依赖「终点锚体系命运」的裁决（g1）；契约一旦定案是后续投影 API（t1）与
结构化输出基建（g4）的前提。
