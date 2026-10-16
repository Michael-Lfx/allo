---
id: q2
title: "quiz 回合载体 + 结算落账 + QuizRecord"
type: task
status: closed
assignee: zhangyj
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

- [x] **载体形态裁决**（本票 Resolution 记录，二选一）：
      方案 A = 瘦回合制（照 mount/repair 先例：POST start → 持题记录 → 逐题 grade →
      POST complete；quiz 状态落新表或复用 attempts；无会话实体）；方案 B = g4 会话
      命令面最小集（start/grade/complete 三件，quiz 为第一种 kind；后续复习/教新
      会话平移）。learnhub 是 B 的参考实现；A 的先例是 mount/repair 回合。
- [x] **存储**：quiz 回合记录落表（迁移 081；回合 = 触发时刻的覆盖节点清单 +
      选题清单 + 学习者作答 + 结算结果 + terminal 状态；**首写为终**——重放不改写
      首跑记录）；表进 v3 契约注册（PRODUCT_TABLES + 逻辑引用）
- [x] **选题**：覆盖节点 = 触发时刻已结算但未 quiz 的节点（K 个，clamp ≤ 预算 15）；
      每节点选 1 道该节点 objective 题（LRU = 最久未出）；无题可选的节点跳过（如实
      缩减覆盖面）
- [x] **脱敏纪律**：start 下发题面 + 选项，**不下发 answer/explanation**——判底只
      随逐题 grade 的反馈下发（复用 `evaluate()` 确定性判分，零 LLM）
- [x] **结算落账**：complete 时逐覆盖节点调 `settle_quiz`（q1 的分级函数）→ 每节点
      一笔 `quiz-completed` 落 `learning_xp_events`（幂等键 course×node×learning-day，
      INSERT OR IGNORE）→ QuizRecord 行落表（全卷 earned 总额 + 逐题对错）
- [x] **同日折叠**：learnhub 语义——quiz 当日已结算的节点 earned 空（coverage 保持
      但不重复计薪）
- [x] **smoke `quiz` 完整化**：结算 K 节点 → 触发 → 程序化作答 → 断言分级结算
      （full/partial 各至少一笔）/恒非负/日幂等（同日重跑 earned 空）/盘面复验照
      repair 模式（EXIT=0 全绿）
- [x] 验证：`cargo test -p nomifun-learning`（quiz 回合全链/选题 LRU/脱敏/首写为终/
      同日折叠）+ smoke 真机 + `bun run typecheck`

**Resolution:**

**速览**：载体裁决 = **方案 A 瘦回合制**；迁移 081 `learning_quiz_rounds` + start/
grade/complete 三命令一次落地（零 LLM，纯银行旧题）；smoke quiz 真机五判据全绿
（EXIT=0：触发门 → 脱敏 → 分级落账 12 XP → 同日折叠 → 首写为终）。

1. **载体形态裁决（票面二选一）**：**方案 A**。理由：① q2 的存储票面（迁移 081
   回合表 + terminal 状态 + 首写为终）本身就是 mount/repair 回合先例的形状，A 是
   顺势而非妥协；② 方案 B 需要先落 g4 会话实体全设计（会话聚合页/复习中心/教新
   复习平移），q3 的弱先/变体/校准不依赖它——现在建会话实体是超前设计；③ A 落表
   比 learnhub 的内存会话簿（`CoachHost.book`，重启即弃、quiz 无放弃面）更耐久，
   「重启清簿」显式化为 start supersede（旧 active 回合标 abandoned，部分唯一索引
   `idx_learning_quiz_rounds_active` 兜并发）；④ 命令面形状（start/grade/complete）
   与 B 同构，将来平移会话实体是重挂载不是重设计。
2. **learnhub 参考比对（开票前已读四文件）**：照抄的——脱敏红线（start 投影判底
   零字段，answer/explanation 只随逐题判分反馈下发，learnhub SessionQuestionView/
   GradeFeedback 同形）、同日折叠（`quiz_settled_on_day`：同课程同节点同学习日只
   结算一笔，coverage 保持 earned 空）、逐覆盖节点一笔 `quiz-completed`（per-node
   聚合 → settle_quiz → append_journal）。**未迁**（范围外，留痕）——① 逐题作答
   ①级证据落账与题级卡推进/建卡（learnhub complete 内做；allo 归 g4 数据面/q3，
   q2 的 complete 只结算+落回合行）；② 弱先全采样与变式配额（q3）。
3. **迁移 081** `learning_quiz_rounds`：一回合一行——`plan_json`（选题快照：
   node/node_name/activity/kind 审计面，**判底不入表**，answer 住
   `learning_activities.config_json`）+ `answers_json`/`report_json`/`earned_xp`
   （completed 时在场）+ 三态 status；CHECK 钉终态形状（active 零结算面/
   completed 必带全量/abandoned 只留情境）。v3 契约注册四处：
   PRODUCT_TABLES（计数 118→119）、UUIDV7_BUSINESS_COLUMNS + NON_REFERENCE_
   ID_COLUMNS（round_id 是业务身份非关系链接）、LOGICAL_REFERENCES（user→users
   Cascade；course→learning_courses **KeepHistory**——行为历史与 XP 账本同律，
   课程删除墓碑后回合原样保留）。
4. **选题（q2 最小面）**：覆盖节点 = 触发时刻已结算未 quiz 的节点（N ≥ K，追加序
   取**最近** budget=15 个——触发量的是新学内容债务，最新节点巩固最浅；q3 换弱先
   全序时此处重裁）；每节点 1 道 objective 题（白名单与教新线同筛，AI 批改轨不入
   quiz——确定性判分是测量面），LRU = attempts ∪ 复习作答事件的最近时点（Option
   自然序：从未出过最先、越久未出越前、activity_id 破平）；无题节点跳过（如实
   缩减覆盖面），全覆盖节点无题 = Conflict 诚实失败。
5. **结算落账**：complete 逐覆盖节点 `settle_quiz`（q1 四档）→ `record_settlement`
   提为 pub(super) 复用（条款先入 + anchor `quiz:{course}:{node}:{学习日}`
   INSERT OR IGNORE + companion 桥；02:00 翻转学习日与复习线同源）→ 回合行
   `UPDATE … WHERE status = 'active'`（rows_affected = 0 → Conflict）。**顺序裁决**：
   先落账后改回合状态——结算失败不产生假完成回合；并发 race 落败方的结算被
   anchor 吸收（不双计），拿 Conflict 账本仍一致。
6. **判分**：`evaluate()` 确定性复用（零 LLM），correct = score ≥ 1.0；grade 纯读
   零落账（learnhub 同纪律：判分不写账，结算在 complete 全卷面）；计划外题判分
   BadRequest；quiz 必作答（complete 缺题点名拒绝）。
7. **验证记录**：`cargo test -p nomifun-learning` 322 全过（+5：开测脱敏/选题 LRU/
   分级+折叠+首写为终/必作答/supersede；q1 的 501 骨架测试改写为真载体测试）；
   `cargo test -p nomifun-db` 458+ 全过（+4 迁移契约：终态形状 CHECK/UUID 地板/
   部分唯一索引）；smoke `quiz` 真机 EXIT=0 五判据全绿；`cargo check --workspace`
   净；fmt 已跑；`bun run typecheck` 仍为同两处存量（videoCanvas/videoGeneration，
   与 q1 记录一致）。
8. **q3 接口备忘**：载体已就位的面——`QuizPlanItem`（plan_json 快照形状，q3 加
   题源/弱项读数字段）；选题循环（弱先排序替换「最近 budget 个」的注释位）；grade
   反馈（q3 变体的判分同径）；`Settlement::settle_quiz` 的 kinds 已支持多题节点
   （q3 每节点 1 旧题 + 1 变体直接生效）；迁移号 082 起未占用。
