---
id: q3
title: "LLM 变体生成 + 弱先排序 + 校准点与读面"
type: task
status: closed
assignee: zhangyj
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

- [x] **弱先排序**：quiz 选题从「纯 LRU」升级为弱先七档（escalation=上卷错题升
      级重出 / non-retention=真保留率低的节点 / lapses=失误多的卡 / 低预测 R /
      untouched=从未 quiz 的节点 / ULID 兜底）——全部从已有账面回放派生，零新存储
- [x] **LLM 变体生成**：每覆盖节点在银行旧题之外生成 1 道变体（基于该节点已教概念
      + 旧题意图换皮）；过内容质量门（9 题型形状 + 答案可判）；生成失败如实跳过
      （该节点只出旧题——与 learnhub「quiz 无 abandon，失败顺延」一致）
- [x] **校准点落盘**：quiz complete 时每题落一个校准点（出题时刻的 FSRS 预测 R 分桶
      vs 实际对错）——喂 #89 归因读面的数据源；落 quiz 记录行（不加新表，扩 q2 的
      QuizRecord 行 JSON）
- [x] **校准读面**：按预测 R 分桶（五百分点）聚合实际正确率——与复习侧
      `MemoryHealthStats` 的预测对照面板同构；quiz 侧校准曲线挂记忆健康或独立端点
      （读数面非门禁）
- [x] **smoke `quiz` 增强**：变体生成的 LLM 调用落观测面档案（照 structure 冒烟
      纪律）；弱先排序断言（造弱节点 → 断言先出）
- [x] 验证：`cargo test -p nomifun-learning`（弱先七档排序/变体生成质量门/校准点
      落盘）+ smoke 真机 + `bun run typecheck`

**Resolution:**

**速览**：弱先七档（账面回放零新存储）+ 变式生成（复用课时加题引擎，无
provider/失败如实跳过）+ 校准点随回合行落盘 + `GET /quiz/calibration` 分桶读面
一次落地；smoke quiz 六判据真机全绿（EXIT=0，含弱先断言与变式诚实跳过）。

1. **弱先七档（learnhub `plan_quiz` 全序的 allo 适配）**：escalation（最近一次
   真实推进 rating=1——learnhub 归因升级的账本等价物，allo 未建归因面）>
   non-retention（节点真保留率 `Σ(review−lapse)/Σreview` 升序，无样本不进档）>
   lapses（`Σ lapse_count` 降序）> 低预测 R（节点卡最小 FSRS 预测升序，
   `predicted_retrievability` 复用调度器单源）> untouched（最近触及时点——
   attempts ∪ 卡推进，从未触及视作最陈旧排最前）> node ULID 兜底。数据源 =
   `learning_review_items`（039 题级卡：lapse_count/review_count/stability_days/
   last_reviewed_at）+ `learning_review_log`（最近真实推进评分）+ attempts——
   全部账面回放派生零新存储，零 LLM。预算 clamp 改取**弱先序前 15 个**（q2 的
   「最近 N 个」注释位兑现重裁）。
2. **变式生成（复用而非新建）**：`generate_lesson_activity` 课时加题引擎直接
   承担变式站职能——同型换皮（kind = 旧题题型；focus 带旧题考点换皮提示）、
   防复用池 = 该节点既有全部题面（`existing_lesson_questions`）、质量门照旧
   （题型形状 + 新颖性 + 可判分），管线 completer 挂 ArchivingCompleter →
   观测档案（smoke 纪律）。**生成失败如实跳过**（该节点只出旧题，warn 留痕；
   completer 缺席同理）——票面语义；matching 题型无变式发生器（跳过）。
   **变式不落 `learning_activities`**（与 learnhub 追加入库的有意分歧）：课时
   内容面零污染——课时练习与教新结算的题量口径不被检测题改写；变式判分底随
   回合行 `plan_json` 落盘（DB 即服务端，判底纪律不变），题号 = 现场铸的 ULID
   （无行）。作答呈现跨节点交错（learnhub 同款，plan 与投影同序）。
3. **校准点**：complete 时逐题一个点（`QuizCalibrationPoint { activity_id,
   correct, predicted_r }`）——预测侧 = 该题题卡的 FSRS 预测可提取性（作答
   时点，stability × elapsed；变式/未建卡 = `None` 不入均值，learnhub
   CalibrationPoint 同形），随 `report_json` 节点条目落盘（**零迁移零新表**，
   `serde(default)` 容 q2 时代旧行）。
4. **校准读面**：`GET /api/learning/quiz/calibration`（独立端点；读数面非
   门禁）——completed 回合的校准点回放，按预测 R 五百分点分桶
   （`recall_bucket` 单源）聚合预测均值 vs 实际正确率，形状复用
   `MemoryCalibrationBin`（与复习侧 `MemoryHealthStats.calibration` 同构）；
   无样本 = 空表不造假数据；坏档行如实跳过。
5. **q2 面的顺延调整**：计划序从「追加序」变「弱先序」（ULID 破平）——q2 的
   两个位置敏感测试改按 activity→node 定位断言；交错序使 plan_json 不再按
   节点分组（complete 的首次出现序 = 弱先序，语义不变）。
6. **验证记录**：`cargo test -p nomifun-learning` 325 全过（+3：escalation 弱先
   / 变式双路（ScriptedCompleter 真生成 + 无 completer 诚实跳过）/ 校准分桶；
   q2 两测试改序无关）；smoke `quiz` 真机 EXIT=0 六判据全绿（观测档案挂载、
   弱先断言、变式诚实跳过、校准 1 桶）；`cargo check --workspace` 净；fmt 已跑；
   `bun run typecheck` 仍为同两处存量（videoCanvas/videoGeneration）。
7. **quiz 线收尾备忘**：三票全关——条款（j05）→ 纯函数+触发（q1）→ 载体+
   落账（q2）→ 质量（q3）全链就位。后置未迁（learnhub 有、allo 未跟）：逐题
   ①级证据落账与题级卡推进/建卡（g4 数据面）、归因面 escalation 的升级/
   撤销通道、quiz 会话记录文件面（allo 落 DB 回合行）。
