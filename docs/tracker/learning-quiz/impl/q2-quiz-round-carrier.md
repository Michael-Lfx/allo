---
id: q2
title: "quiz 回合载体 + 结算落账 + QuizRecord"
type: task
status: open
assignee:
blocked_by: [q1]
part_of: ../map.md
---

# q2: quiz 回合载体 + 结算落账 + QuizRecord

**What to build:** quiz 的作答载体与结算落账——从触发到完成的全链路。载体形态本票裁
决（方案 A 瘦回合制 vs 方案 B 完整会话实体，见 Resolution 段）。零 LLM——纯银行旧题。

**实现决议:** 三轮 t1 + learnhub `app/sessions.rs`（quiz 会话命令面/脱敏投影）+
`domain/sessions.rs`（SessionRecord 首写为终）+ `domain/quiz.rs`（选题预算）+
`engine/coach.rs`（quiz 会话完成→settle_quiz 落账）。**开票前先读参考代码**。

**Reference（新会话第一步：按序读完再动 allo 代码）**

| 序 | 文件（learnhub） | 看什么 |
|---|---|---|
| 1 | `src-tauri/src/app/sessions.rs` | quiz 命令面（start 后持题脱敏——判底只随 grade 下发；complete 整卷结算）；`SessionView` 字段级缺席 |
| 2 | `src-tauri/src/domain/sessions.rs` | `SessionRecord` 首写为终（write_session_record 已存在 = 保留首份）；abandoned 语义 |
| 3 | `src-tauri/src/domain/quiz.rs` | 选题：每覆盖节点 1 道银行旧题（LRU）+ 预算 clamp 15；弱先排序 |
| 4 | `src-tauri/src/engine/coach.rs` | quiz 会话完成路径：逐覆盖节点 settle_quiz + append_journal + QuizRecord 落盘 |

**Blocked by:** [q1]。

**Status:** ready-for-agent

- [ ] **载体形态裁决**（本票 Resolution 记录，二选一）：
      方案 A = 瘦回合制（照 mount/repair 先例：POST start → 持题记录 → 逐题 grade →
      POST complete；quiz 状态落新表或复用 attempts；无会话实体）；方案 B = g4 会话
      命令面最小集（start/grade/complete 三件，quiz 为第一种 kind；后续复习/教新
      会话平移）。learnhub 是 B 的参考实现；A 的先例是 mount/repair 回合。
- [ ] **存储**：quiz 回合记录落表（迁移 081；回合 = 触发时刻的覆盖节点清单 +
      选题清单 + 学习者作答 + 结算结果 + terminal 状态；**首写为终**——重放不改写
      首跑记录）；表进 v3 契约注册（PRODUCT_TABLES + 逻辑引用）
- [ ] **选题**：覆盖节点 = 触发时刻已结算但未 quiz 的节点（K 个，clamp ≤ 预算 15）；
      每节点选 1 道该节点 objective 题（LRU = 最久未出）；无题可选的节点跳过（如实
      缩减覆盖面）
- [ ] **脱敏纪律**：start 下发题面 + 选项，**不下发 answer/explanation**——判底只
      随逐题 grade 的反馈下发（复用 `evaluate()` 确定性判分，零 LLM）
- [ ] **结算落账**：complete 时逐覆盖节点调 `settle_quiz`（q1 的分级函数）→ 每节点
      一笔 `quiz-completed` 落 `learning_xp_events`（幂等键 course×node×learning-day，
      INSERT OR IGNORE）→ QuizRecord 行落表（全卷 earned 总额 + 逐题对错）
- [ ] **同日折叠**：learnhub 语义——quiz 当日已结算的节点 earned 空（coverage 保持
      但不重复计薪）
- [ ] **smoke `quiz` 完整化**：结算 K 节点 → 触发 → 程序化作答 → 断言分级结算
      （full/partial 各至少一笔）/恒非负/日幂等（同日重跑 earned 空）/盘面复验照
      repair 模式（EXIT=0 全绿）
- [ ] 验证：`cargo test -p nomifun-learning`（quiz 回合全链/选题 LRU/脱敏/首写为终/
      同日折叠）+ smoke 真机 + `bun run typecheck`

**Resolution:**（收口时写）
