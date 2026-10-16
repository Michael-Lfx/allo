---
id: q1
title: "settle_quiz 分级函数 + K 计数触发 + N/K 读数"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# q1: settle_quiz 分级函数 + K 计数触发 + N/K 读数

**What to build:** quiz 线的纯函数面与触发读数——`settle_quiz` 分级函数移植进
`settle.rs`（零 IO，红绿样钉住）、K 计数从 `learning_xp_events` 回放派生（零新存储）、
每课程 N/K 进度读数暴露。本票零 LLM 可全验。

**实现决议:** 三轮 t1 + learnhub `engine/questions/settle.rs`（settle_quiz 函数）+
`domain/quiz.rs`（触发计数）。**开票前先读参考代码**——learnhub 侧 quiz 线已完整实现，
本票是移植不是设计。

**Reference（新会话第一步：按序读完再动 allo 代码）**

| 序 | 文件（learnhub） | 看什么 |
|---|---|---|
| 1 | `src-tauri/src/engine/questions/settle.rs` | `settle_quiz`：worked = Σ 当量（**无 3 分钟下限**——与 node/review 阶梯的关键区别）；分级 = ≥0.9 worked+bonus / ≥0.6 worked / ≥0.3 worked÷2 / else worked÷4；quiz 恒非负；零题拒绝 |
| 2 | `src-tauri/src/domain/quiz.rs` | `QUIZ_TRIGGER_SETTLEMENTS = 6`（K 起步值待校准）；计数 = node-completed since last quiz-completed per course（复习账不推进）；`QUIZ_QUESTION_BUDGET = 15` |
| 3 | `src-tauri/src/infra/ledger/event.rs` | quiz 结算事件的形状与 validate（恒非负条款的原文位置） |

**Blocked by:** 无（账本与条款 j05 已就位）。

**Status:** ready-for-agent

- [ ] **settle_quiz 分级函数**进 `settle.rs`（零 IO 纯函数 + 红绿样）：
      `worked = Σ(该节点本次 quiz 题目的工作量当量)`（复用 `question_equivalent`，
      **无 `REVIEW_MIN_MINUTES` 下限**）；输入 = covered node 的题目当量列表 + 准确率
      （答对数÷题数）；分级 = accuracy ≥ 0.9 → worked+`XP_FULL_BONUS`（full）/
      ≥ 0.6 → worked（full）/ ≥ 0.3 → worked÷2（partial, ratio=accuracy）/
      else → worked÷4（partial, ratio=accuracy）；零题 = 拒（fail loud）；输出过
      `validate_settlement("quiz-completed", …)` 零放宽
- [ ] **K 计数派生**：`LearningService` 新增纯读方法——按课程从
      `learning_xp_events` 回放 `COUNT(kind='node-completed') - COUNT(kind=
      'quiz-completed')`，clamp ≥ 0（复习/检测账不推进触发进度）；零新存储
- [ ] **N/K 读数**：`QuizProgressView { settled_count, trigger_count, due: bool }`
      挂在课程视图或独立端点 `GET /courses/{id}/quiz/progress`；读数面非门禁
- [ ] **触发命令骨架**：`POST /courses/{id}/quiz/start` 服务面骨架——due=false 时
      Conflict（附 N/K），due=true 时本票只返回 501 Not Implemented 占位（q2 接）
- [ ] **quiz 冒烟骨架**：learning-smoke 增 `quiz` 子命令占位（结算 K 节点 → 断言
      due=true → 打印 N/K）；完整判据随 q2
- [ ] 验证：`cargo test -p nomifun-learning`（settle_quiz 四档红绿样/恒非负/零题拒/
      K 计数边界=恰 6 触发、复习不推进、quiz 自身不推进）+ `bun run typecheck`

**Resolution:**（收口时写）
