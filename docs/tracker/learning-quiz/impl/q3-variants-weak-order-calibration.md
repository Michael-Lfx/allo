---
id: q3
title: "LLM 变体生成 + 弱先排序 + 校准点与读面"
type: task
status: open
assignee:
blocked_by: [q2]
part_of: ../map.md
---

# q3: LLM 变体生成 + 弱先排序 + 校准点与读面

**What to build:** quiz 线的质量增量——变体题生成、弱节点先出的选题排序、校准点
落盘与读面（#89 校准曲线的数据源）。

**实现决议:** learnhub `domain/quiz.rs`（弱先排序七档）+ `engine/coach.rs`（变体
生成 + CalibrationPoint）+ `engine/questions/grading.rs`（确定性判分——allo 已有
`evaluate()`）。**开票前先读参考代码**。

**Reference（新会话第一步：按序读完再动 allo 代码）**

| 序 | 文件（learnhub） | 看什么 |
|---|---|---|
| 1 | `src-tauri/src/domain/quiz.rs` | 弱先排序七档：escalation > non-retention > lapses > 低预测 R > untouched > ULID（确定性的，无 LLM） |
| 2 | `src-tauri/src/engine/coach.rs` | 变体生成：基于银行旧题生成变体（LLM 调用 + 质量约束）；CalibrationPoint：出题时刻预测 R vs 实际对错 |
| 3 | `src-tauri/src/engine/questions/grading.rs` | 确定性判分零 LLM（allo `evaluate()` 已同构，确认无缺口） |

**Blocked by:** [q2]。

**Status:** ready-for-agent

- [ ] **弱先排序**：quiz 选题从「纯 LRU」升级为弱先七档（escalation=上卷错题升
      级重出 / non-retention=真保留率低的节点 / lapses=失误多的卡 / 低预测 R /
      untouched=从未 quiz 的节点 / ULID 兜底）——全部从已有账面回放派生，零新存储
- [ ] **LLM 变体生成**：每覆盖节点在银行旧题之外生成 1 道变体（基于该节点已教概念
      + 旧题意图换皮）；过内容质量门（9 题型形状 + 答案可判）；生成失败如实跳过
      （该节点只出旧题——与 learnhub「quiz 无 abandon，失败顺延」一致）
- [ ] **校准点落盘**：quiz complete 时每题落一个校准点（出题时刻的 FSRS 预测 R 分桶
      vs 实际对错）——喂 #89 归因读面的数据源；落 quiz 记录行（不加新表，扩 q2 的
      QuizRecord 行 JSON）
- [ ] **校准读面**：按预测 R 分桶（五百分点）聚合实际正确率——与复习侧
      `MemoryHealthStats` 的预测对照面板同构；quiz 侧校准曲线挂记忆健康或独立端点
      （读数面非门禁）
- [ ] **smoke `quiz` 增强**：变体生成的 LLM 调用落观测面档案（照 structure 冒烟
      纪律）；弱先排序断言（造弱节点 → 断言先出）
- [ ] 验证：`cargo test -p nomifun-learning`（弱先七档排序/变体生成质量门/校准点
      落盘）+ smoke 真机 + `bun run typecheck`

**Resolution:**（收口时写）
