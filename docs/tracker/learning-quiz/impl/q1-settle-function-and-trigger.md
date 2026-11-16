---
id: q1
title: "settle_quiz 分级函数 + K 计数触发 + N/K 读数"
type: task
status: closed
assignee: zhangyj
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

- [x] **settle_quiz 分级函数**进 `settle.rs`（零 IO 纯函数 + 红绿样）：
      `worked = Σ(该节点本次 quiz 题目的工作量当量)`（复用 `question_equivalent`，
      **无 `REVIEW_MIN_MINUTES` 下限**）；输入 = covered node 的题目当量列表 + 准确率
      （答对数÷题数）；分级 = accuracy ≥ 0.9 → worked+`XP_FULL_BONUS`（full）/
      ≥ 0.6 → worked（full）/ ≥ 0.3 → worked÷2（partial, ratio=accuracy）/
      else → worked÷4（partial, ratio=accuracy）；零题 = 拒（fail loud）；输出过
      `validate_settlement("quiz-completed", …)` 零放宽
- [x] **K 计数派生**：`LearningService` 新增纯读方法——按课程从
      `learning_xp_events` 回放 `COUNT(kind='node-completed') - COUNT(kind=
      'quiz-completed')`，clamp ≥ 0（复习/检测账不推进触发进度）；零新存储
- [x] **N/K 读数**：`QuizProgressView { settled_count, trigger_count, due: bool }`
      挂在课程视图或独立端点 `GET /courses/{id}/quiz/progress`；读数面非门禁
- [x] **触发命令骨架**：`POST /courses/{id}/quiz/start` 服务面骨架——due=false 时
      Conflict（附 N/K），due=true 时本票只返回 501 Not Implemented 占位（q2 接）
- [x] **quiz 冒烟骨架**：learning-smoke 增 `quiz` 子命令占位（结算 K 节点 → 断言
      due=true → 打印 N/K）；完整判据随 q2
- [x] 验证：`cargo test -p nomifun-learning`（settle_quiz 四档红绿样/恒非负/零题拒/
      K 计数边界=恰 6 触发、复习不推进、quiz 自身不推进）+ `bun run typecheck`

**Resolution:**

**速览**：settle_quiz 四档移植 + K 计数回放 + N/K 读数 + start 骨架 + 冒烟骨架
一次落地，零迁移零新存储。真机冒烟三判据全绿（5/6 未到期 → 6/6 到期 → 501 占位）。

1. **参考差异入账（开票前已读 learnhub 三文件）**：quiz 线与 node/review 阶梯的
   关键区别——①基数无 `REVIEW_MIN_MINUTES` 下限（learnhub `session_work_minutes`
   原样作基数，复习线是 `max(…, 3)`）；②授予按准确率四档而非 award 比例阶梯
   （0.9/0.6/0.3 门槛常量集中 settle.rs，与 GUESS_* 同一「报警阈值可调」纪律）；
   ③零题拒绝（测量语义：无准确率可分级——与 node 线「无练习课时 = full 全额」
   相反）+ 题单/作答数错配与答对数越界 fail loud（错配即错价）。
2. **`Settlement::settle_quiz(kinds, correct, answered)`**（零 IO，返回
   `Result<Self, String>`）：worked = Σ `question_equivalent`；四档授予照抄
   learnhub 真值表（4 单选 worked 4 → 6/4/2/1/1）；红绿样五组，每组输出过
   `validate_settlement("quiz-completed", …)` 零放宽；「无下限」有专项对照样
   （2 单选全对 = 4，复习线同型是 5）；「恒非负」对照样（8 全错 = +2，教新线
   同结局是 −12 的乱猜罚）。Trimmed 形态留给 q2 载体按需构造（`Settlement` 是
   pub struct，砍短 = 恰 0 的公开构造，q1 命令面用不到）。
3. **K 计数语义分歧裁决（票面公式的偏差）**：票面写 `COUNT(node) − COUNT(quiz)`
   clamp ≥ 0，但 learnhub 参考实码（map.md 定的最高事实源）是「**上次
   quiz-completed 之后**的 node-completed 数」（追加序权威，quiz 即清零）。两者
   只在「每次积累满 K 必 quiz」的纪律性使用下等价；用户 quiz 后新学 2 节点时差
   公式给出 K+1 误报 due。**取参考实码语义**：SQL 单查询 `NOT EXISTS (quiz 行
   rowid > node 行 rowid)`（账本 `id INTEGER PRIMARY KEY AUTOINCREMENT` 即追加
   序，与 learnhub journal 追加序同构），COUNT 天然非负。测试钉死：恰 6 触发、
   复习账不推进、quiz 清零重新积累、未知课程 NotFound。
4. **N/K 读数**：`QuizProgressView { settled_count, trigger_count, due }`（models.rs，
   Serialize）挂独立端点 `GET /api/learning/courses/{id}/quiz/progress`（与 start
   同族，不挤课程视图）；课程不存在/已删墓碑 = NotFound，读数面非门禁。
5. **start 骨架**：`POST /api/learning/courses/{id}/quiz/start`——due=false =
   Conflict 409（消息附 N/K），due=true = 501 Not Implemented 占位。501 需要新
   `AppError::NotImplemented` 变体（nomifun-common：status NOT_IMPLEMENTED、
   code "NOT_IMPLEMENTED"；nomifun-app-server 的 AppError 穷举 match 补
   `("not_implemented", false)` 映射——与 browser_management 直用 501 的先例
   同源）。handler 返回形状按 q2 将来的回合视图预留（`ApiResponse<QuizProgressView>`，
   q2 替换）。
6. **冒烟骨架**：`nomicore learning-smoke quiz`（零 LLM，缺省从零临时库）——种
   K 节点课程（est 25），真实管线逐节点结算，机械判据 ①K−1 后 due=false ②恰 K
   后 due=true ③start 到期时 501 占位。已在临时库真机跑通三判据全绿。
7. **验证记录**：`cargo test -p nomifun-learning` 318 全过（含 settle_quiz 五组
   红绿样 + 两个 K 计数服务测试）；`cargo test -p nomifun-common` 275 全过
   （AppError 新变体）；`cargo check --workspace` 过；fmt 按包跑过。
   `bun run typecheck` 挂存量失败（`videoCanvas`/`videoGeneration` 两文件，
   last touched c28ab82cf，不在本票改动集——沿「存量失败别误判」纪律记录不修）。
8. **q2 接口备忘**：载体票直接消费的面——`settle_quiz`（作答折叠后调）、
   `quiz_settled_count`（结算后自然清零）、`QuizProgressView`（start 成功路径
   替换占位）、`QUIZ_QUESTION_BUDGET`（贪心装填护栏）；迁移号 081 起未占用。
