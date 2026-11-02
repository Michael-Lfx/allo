---
id: w2
title: "练习题型接核验器——numeric 答案键工具算/变式换参/多步批改"
type: task
status: closed
assignee: zhangyj
blocked_by: [w1, h1]
part_of: ../map.md
---

# w2: 练习题型接核验器

**What to build:** quiz/练习线的可计算量全部改由 verify_math 产出：numeric 题
答案键工具算（tol 只承担数值噪声语义，不再背书模型口算）；q3 变式生成升级为
「换参数 + 工具算新答案键」；多步题 AI 批改对作答中间量逐个核验（部分分依据）。

## 用户动机（2026-10-31 拍板原话要点）

核验基建是之后升级练习题型的地基——本票就是那个「升级」本体：
- numeric 题答案键今天靠模型报数（无确定性背书）；
- q3 变式是「同型换皮」，参数与答案键同为模型生成；
- AI 批改轨对多步作答没有逐步核验的抓手。

## allo 现状（落点提示，实现会话复核）

- quiz 线：`settle.rs` 四档判分（numeric tol 判分、0.9/0.6/0.3 门槛常量集中）；
  q2 回合载体（`learning_quiz_rounds`，plan_json 判底）；q3 变式生成（复用
  generate_lesson_activity 课时加题引擎 + 防复用池 + ArchivingCompleter）。
- verify_math 求值器落 nomifun-learning（w1 已定）——settle/变式同 crate，
  复用最近。

## 实施要点

- [ ] numeric 题生成侧：模型只给参数化表达式与参数值，答案键必须经 verify_math
      eval 实算落库；题面渲染与答案键同源（同一表达式两处消费）。
- [ ] 变式生成：参数化模板换参 → 新答案键工具算；防复用池语义保持（题面变化
      但同模板的判定不变）；变式不落 learning_activities 的既有裁决不变。
- [ ] 多步题 AI 批改轨：作答中间量逐 claim 核验，部分分 = 已核验通过步骤的
      权重；判分反馈带核验细节（哪步对哪步错）。
- [ ] 红绿样：答案键与工具输出一致的断言（含舍入路径）；变式换参后答案键变化
      且精确；批改部分分的边界。

## 验收

- `cargo test -p nomifun-learning` 全绿；fmt；零迁移（如需存表达式+参数则
  另评估，倾向复用现有 JSON 列）。
- 真机判据：quiz smoke numeric 题答案键与工具输出一致；变式题换参后答案精确
  且判分正确。

## 非目标

- 选择/判断/匹配等非计算题型（无 oracle 需求，现状已够）；
- 主观题语义判分（AI 批改轨的 LLM 判断部分不动，本票只加确定性核验层）。

## Resolution（2026-10-31 grilling 裁决：范围收窄）

**裁决（用户：按推荐）**：w2 收窄为三件——**numeric 答案键工具算 + 变式换参数
+ 修复变式 `tol: None` 硬编码缺陷**；「多步题型 + 部分分」**剥离为后置票**
（触发条件：产品要求数学多步题）。等 w1 真机判据落地再动（隔波）。

**影响面核查勘误（2026-10-31 实码核实）：**

- **现状真缺陷**：变式 numeric `tol: None` 硬编码（quiz.rs:819）→ evaluate 按
  1e-9 精确判（progress.rs:482-494，绝对容差）——模型口算误差直接判错学习者。
  w2 换工具算同时修复。
- **「多步题+部分分」无载体**：九题型（models.rs:1464-1481）无多步形态；AI 批改是
  单文本整体评分（open_judge.rs）；settle 部分分通道（settle_review ratio）生产恒收
  0/1（xp.rs:132）；quiz 线纯 objective（选题 SQL 白名单）是有意设计——那是新题型
  设计，非「接核验器」。
- **票面措辞勘误**：settle.rs 不含 tol 比对，四档（0.9/0.6/0.3）是**整场准确率**
  授予门槛而非单题部分分；题号铸的是 **UUIDv7**（LearningActivityId）非 ULID。
- smoke quiz 现只种 single_choice（learning_smoke.rs:1916-1944）——w2 需新增
  numeric 种子题 + grade 断言（2090-2124）才有真机判据落点。
- numeric 全链：生成条款 generation/mod.rs:114（answer JSON number + tol）/:129
  自检；ActivityPack models.rs:1429-1462；落库 config_json（StoredActivityConfig
  3350-3377，serde default 容旧行→**零迁移可扩表达式+参数字段**）；
  LESSON_ACTIVITY_SYSTEM（mod.rs:142-166）**无 numeric 形状条款**，换参契约要新增；
  变式单次 LLM 调用一次给题面+答案，无参数分离（quiz.rs:768-852）。

## Resolution 增补（2026-11-02，learnhub `b6a8c4e` 数学线复核）

**背景**：上次引用 learnhub 基线是 `bd31277`（e2e-fixes 波）。learnhub 在
`bd31277..b6a8c4e` 新增 21 提交，其中 13 是数学线（ADR-0038 + #186/#187/#188 +
`c14467e` 硬化 + #195 铸值 + #196 写侧工具 op）。w2 从「无参考实码」变成
「有完整判底铸值参考」。完整影响面、逐文件落点与决策矩阵见
[assets/2026-11-02-w2-numeric-cast-impact.md](../assets/2026-11-02-w2-numeric-cast-impact.md)。

**裁决（按推荐，2026-11-02）：**

1. **`answer` 保持 JSON number（显示单位）**，不改成 learnhub 式显示字符串。
   判分臂 `progress.rs:515-528`、smoke `item_answer`、编辑回传都不动。
2. **claim 形状 = `{expr, params}`**（不是 learnhub 的单表达式）：支持 w2 原始
   动机「同模板换参」；求值走 `eval_expr(expr, bindings)`。
3. **系统铸值**：模型 numeric 轨只给 `claim + precision(0–10) + unit(raw|percent)`，
   **禁写 `answer`/`tol`**；`cast_numeric_claim` 实算并写入 `answer`（percent 存
   显示单位 ×100）与系统 tol（精确整数 = 0；否则 `0.5×10^-precision`）。
4. **claim 只在生成面/服务端结构（`ActivityPack` + `StoredActivityConfig`）**；学习者
   DTO 只加 `precision/unit`，零 `claim`。管理面 claim 轨的 answer 不可手改；
   编辑保留三键与 tol。
5. **变式路跳过题面相似门**（同考点换面是设计目的）；课时加题/重出的防复用池
   语义不变。变式 `tol` 由铸值给出，不再 `None`。
6. **h1 先行**：w2 的 claim 表达式直接进求值器，2048 字符/64 层预算必须先落地；
   本票 `blocked_by: [w1, h1]`。
7. **v0 边界**：手工/自定义 numeric 仍走 legacy 手写 answer；不做题干模板渲染
   （只保证答案键同源）；不吸收 claims 围栏；多步题/部分分仍在雾区。

**实施要点（2026-11-02 版，覆盖上方旧清单中冲突项）：**

- [ ] `verify_math.rs` 新增 `NumericClaim { expr, params: BTreeMap<String,f64> }`、
      `NumericUnit { Raw, Percent }`、`cast_numeric_claim(...) -> CastNumeric{answer,tol}`。
- [ ] `ActivityPack` / `StoredActivityConfig` 增 `claim/precision/unit`（serde default，
      零迁移）；五处 `StoredActivityConfig` 构造统一走 constructor。`ActivityPack`
      加字段会打断约 20 处测试字面量（`generation/tests.rs`、`service/tests.rs`、
      `lesson_draft.rs` 测试 helper 等）——统一补 `claim: None, precision: None,
      unit: None`，或引入测试构造器。
- [ ] 铸值接线：整课生成 `activities.rs:203-215`、单题 `:313-315`、agent 草稿
      `lesson_draft.rs:770-790` + `refresh_audit` 兜底、落库 `lesson.rs:2154-2168`、
      导入 `course.rs:162-190`、变式 `quiz.rs:875-890`。
- [ ] 模型面提示词：`generation/mod.rs:114`（LESSON_SYSTEM numeric 条款）、
      `:142-166`（LESSON_ACTIVITY_SYSTEM 新增 numeric 条款）、
      `lesson_content_loop.rs:134/1153/164`（中文 agent 双宿 + 修复表）；
      加条款覆盖断言（无 golden）。
- [ ] 变式：`generate_quiz_variant` 传 bank config；`GenerationPurpose::Variant`
      跳过相似门；`VariantPayload.config_json` 带 claim/precision/unit/tol；
      `QUIZ_ROUND_DOC_VERSION=1` 不 bump。
- [ ] DTO/UI：`ActivityView` / `ReviewQuestion` / `QuizRoundQuestionView` /
      `QuestionEntry` / `GeneratedLessonActivity` 加 `precision/unit`；
      `QuizGradeFeedback` / `ReviewAnswerResult` 揭示格式化；`ActivityInput`
      percent 后缀；`reviewAnswerText` 精度/单位；`types.ts` 同步。
- [ ] 判分/编辑：numeric 臂语义不变；加 exact/within/outside/legacy 测试；
      `update_question` 保留 claim/precision/unit/tol，claim 轨 answer 手改拒。
- [ ] smoke：numeric 种子 + `item_answer` 按 config + 泄漏断言扩 claim/tol +
      变式重算 + XP 按 `question_equivalent(kind)` 求和（numeric 权重 2）。
- [ ] 红绿样：cast（raw/percent/params/精确整数/非有限/未绑定）、生成 claim-only
      与带 answer 重试、agent 草稿 audit、变式换参、legacy 旧行、DTO 无 claim。

**验收（2026-11-02 版）：**

- `cargo test -p nomifun-learning`、`cargo test -p nomifun-ai-agent` 全绿；fmt。
- `bun run typecheck`；若新增 i18n key 则 `bun run gen:i18n && bun run check:i18n`。
- 零迁移：`git diff -- crates/backend/nomifun-db/migrations` 必须为空。
- 真机：`learning-smoke quiz`（建议新增 `quiz-numeric`）numeric 题答案键与铸值
  一致；变式换参后答案变化且精确；start 投影无 claim/tol；XP 断言按题型权重。
- 如实记录：题干措辞与 claim 的「同源」只到提示词条款 + smoke 抽验，不做模板
  渲染（见 D7）；learnhub 的 false 打回路径也从未自然触发，allo 的 smoke 要
  主动构造 outside-tol 反例。

**与旧 Resolution 的关系**：2026-10-31 的收窄裁决（三件：答案键工具算 + 变式换参
+ tol 修复）继续有效；本增补只把「工具算」落成 claim/params + 系统铸值，并把
多步题/部分分继续留在雾区。旧清单中「题面渲染与答案键同源（同一表达式两处消费）」
按 D7 解释为「答案键同源」，不是题干模板渲染。

## Resolution（2026-11-02 施工关闭）

**交付**：

- `dbca554f1`（w2 实施，分支 `feat/verify-math-w2`）→ 合并 `696278e44`。
- `43748212f` + `d58f15fbb`（双轴 code-review 修复，分支
  `feat/verify-math-review`）→ 合并 `90e8215ce`（集成分支
  `feat/verify-math-cast` 尖端，已推送）。

**落地**：

- `answer` 保持 JSON number（显示单位；percent ×100），判分通道
  `progress.rs` 语义不动；`claim{expr,params}` + `precision(0–10)` +
  `unit(raw|percent)` 落 `ActivityPack`/`StoredActivityConfig`，零迁移。
- `cast_numeric_claim` 系统铸值：percent ×100、精确整数 + precision 0 → tol 0、
  否则半显示位；`Strict` 模型面必须带 claim 且禁写 answer/tol，`Force` 导入/
  落库/审计兜底保留 legacy 无 claim 行。
- 变式：bank config 进提示词；`GenerationPurpose::Variant` 跳过题面相似门；
  机械契约要求同 `expr`（空白归一）、同 effective unit、同 precision、参数
  必须变、铸值答案必须变，不合格如实跳过；`tol` 不再 `None`。
- DTO：`ActivityView`/`ReviewQuestion`/`QuizRoundQuestionView`/`QuestionEntry`/
  `GeneratedLessonActivity`/`QuizGradeFeedback`/`ReviewAnswerResult` 只加
  `precision/unit`，`claim` 永不出现；泄漏断言覆盖 lesson/review/quiz start。
- 编辑面：`update_question` 保留 claim/precision/unit/tol；claim 轨 answer
  按数值等价比较（`25.0` / `25` 往返不算手改），真正改值仍拒绝。
- 提示词 EN/CN 统一为「eval_expr 文法（显式 *、^、函数、pi/e）」，修复函数表
  漂移；变式 prompt 使用专用「同模板换参」段落。
- UI：`ActivityInput` percent 后缀；`reviewAnswerText` 按 precision/unit 揭示；
  无新增 i18n key。
- smoke：`learning-smoke quiz` 节点 0 numeric claim 种子；答案键按 config 重算；
  精确/容差内/容差外、变式重算、start 投影无 claim/tol、XP 按
  `question_equivalent(kind)` 求和（numeric 权重 2）。

**验证**：

- `cargo test -p nomifun-learning`：**799 passed, 0 failed**。
- `cargo test -p nomifun-ai-agent --lib lesson_content_loop`：**28 passed**。
- `cargo check -p nomifun-app` 通过；`bun test src/renderer/pages/learning`
  **64 passed**；`bun run check:i18n` up to date（11541 keys / 54 modules）。
- 真机 `learning-smoke quiz`：**EXIT 0**；`[numeric] cast answer=25 tol=0.05
  (exact/within/outside all correct)`；`settled=6 earned=13`。
- 零迁移：`git diff -- crates/backend/nomifun-db/migrations` 为空。
- `bun run typecheck` 仍有 54 个仓库既有错误（videoCanvas/auth 等），
  `pages/learning` 零新增错误。

**如实记录（残留）**：

- 真机 smoke 未带 provider，变式生成如实跳过；变式契约由脚本化 completer
  服务测试覆盖，未在真机 LLM 上自然触发。
- 题干措辞与 claim 的「同源」只到提示词条款 + smoke 抽验，不做模板渲染（D7）。
- claims 围栏、多步题/部分分、手工/自定义 numeric claim 作者面仍在非目标/雾区。
- 集成分支未并回 `feat/learn0928`；按用户合并纪律等通知。
