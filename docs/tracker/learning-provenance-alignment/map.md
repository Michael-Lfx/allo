# Map: 出处门与衍生缺口波（pg0–pg5）

<!-- wayfinder:map -->
<!-- 状态：**已施工关票**（2026-11-26）。触发=隔壁 learnhub 2026-11-25/26 的
      33 提交（#275–#293，main@52e95cb）合入。复核结论：唯一新机制是出处门
      （#287–#293）；12 个非门票里 9 票是本仓已有/反向吸收，3 票
      （#275/#278/#286）照出了本仓 t1/t3/c1 没收掉的衍生缺口。已按影响面
      复核立 pg0–pg5 六票并全部关票：Stage 0/1/1c + 三个衍生
      小修；Stage 2 先量后裁（pg0 样本 0 翻转），Stage 3 不搬。后端
      1137/0、UI 学习页 529/0、typecheck/check 绿；pg1/pg2/pg4 的真机
      校准与拆读欠账见各票 Resolution。 -->

## 一、隔壁这波是什么（2026-11-26 复核）

- **出处门**（#287 父票 + #288 Stage 0 + #289 Stage 1 + #290 Stage 1c +
  #291 Stage 2 + #292 Stage 3 + #293 文档，约 5k insertions）是唯一新
  机制：把「每批每词的语义审判」改成「引用表 + 机械解析 + 一次入账」。
  收口读数：回放 A/B 10/10 正向；从零课真机 5 节点 / 11 次调用 /
  ≈5 分钟（病历：0 节点 / 31 调用 / 888s）。
- **其余 12 票逐票对照**（票面「来源」多为「allo 第三轮对照侦察
  2026-11-24」）：

| # | learnhub 内容 | 本仓状态 | 动作 |
| --- | --- | --- | --- |
| 275 | 删课收尾在途任务/会话/迟到写 | t3 只收 quiz 回合与死卡；生成任务/课时草稿/迟到写未收 | **pg3** |
| 276 | `:::svg` 固定浅色画布 | `learning-svg-figure-canvas-wave` 已落 | 不搬 |
| 277 | 逐调用耗时/token/任务汇总 | rs1 档案已有 `elapsed_ms`+`usage`；缺一行汇总与任务行 token | 小件 |
| 278 | 归档课检测读数到期 | 聚合已筛 `enabled`；按课读数/start 未筛；UI 已藏 | **pg5** |
| 279 | `ng.refs` 打回给可照抄补条 | m1 已有逐名补条 | 不搬 |
| 280 | 教材写作四规则 | pf4 已在 agent 线；传统线缺（内部一致性） | 小件 |
| 281 | 回放放行锚/多跑读数 | rs5 已有回放夹具与多跑读数 | 不搬 |
| 282 | 任务「已用时」+批计数 | f1 已有时长（pill 走表）；无批计数 | 小件 |
| 283 | 评审传输失败立即终止 | rs4 是独立传输额度再试一次（本仓已裁口径） | 不搬 |
| 284 | 评审前预算判定点 | rs8 已有「每次 attempt 前/评审前」判定 | 不搬 |
| 285 | 题面自足/范围越界读数 | sq 已有且更严（danger 阻断） | 不搬 |
| 286 | 已学完仍可置跳过 | 本仓同病（课时级），且卡片分子分母分裂 | **pg4** |

## 二、出处门：本仓同构面（对码 2026-11-26）

### 名池：一个平集合（四刀的共同根）

`crates/backend/nomifun-learning/src/service/learning_graph.rs:3851-3870`：

- `coverage` = 全部存活课程 teaches（`:3626-3638`）+ 已会声明概念
  （`:3649-3668`，按 `ConceptTier::Teach` 并入）；
- `registry_names` = 全库 `learning_concept_registry WHERE deprecated=0`
  的 canonical + 活跃别名（`:3672-3690`、`:5504-5518`，无课程过滤）；
- `provenance_known = coverage.keys() ∪ registry_names ∪ 本批名`
  （`:3854-3870`）——一个 `HashSet<String>`，无来源标签。

### 四刀现状

| 刀 | 现状 | 证据 |
| --- | --- | --- |
| `已教` = covered | 否，在册也算 | `provenance.rs:298-316` 只查 `known.contains(ref)` |
| 直接出处 term==ref/别名 | 否 | 同上；测试 `anchored_row_keeps_taught_ref_semantics`（`provenance.rs:917-935`）以 term≠ref 过门 |
| 自教同节点 | 否，全批并集 | `provenance.rs:190-199`；提示词 `mod.rs:1055`（本节点）vs `:1059`（本批某节点）文/码不一致 |
| 声明独立来源 | 否，并入已教 | `TERM_SOURCES`（`provenance.rs:35`）；`service/learning_graph.rs:3649-3668` |

### 声明地板：存了块，没消费

`learning_basis_declarations.blocks_json` 存声明块 slug
（`service/learning_graph.rs:6335` 附近），消费者只取 concept_ids
（`:4810`、`:5889`、`:6008`）。首批地形清单由
`build_coach_context` → `render_terrain_list` 装配
（`:3299-3338`、`structure_view.rs:644-668`）；声明块无节点 → 永远展开。
首批「不重教」只有提示词软句（`mod.rs:741-744`），没有机械减法。

### Stage 2/3 的边界

- 无 derivation 表 / admission 账本 / `subsumed|lexicalized|plain` /
  `primitive|ostensive`（全 crate 零命中）。
- 无链接器：`learning_graph/mod.rs:686` 明写「不做定向补源调度」。从零
  首批由「空账 ⇒ assumes 全空」机械地板兜底（`mod.rs:433-447`），不存在
  learnhub 那条「定义闭包递归补源 → 600s 耗尽 → 0 节点」的病灶。
- 因此 Stage 3 的 primitive/ostensive/bootstrap 不搬；Stage 2 先量后裁。

### 隔壁 Stage 0 的真机读数（只作风险先验，不是本仓读数）

4068 行 / 819 已教 / 2691 常识 / 371 缺失 / 187 自教；`已教 term≠ref`
263（32.1% of 已教）；在册未教 109（13.3%）；兄弟供给 23（12.3% of
自教）；跨批翻转 268 对；纸面提名 9 行。本仓是否同量级 = pg0 的事。

## 三、衍生缺口（这波顺带照出的本仓残余）

### pg3 删课收尾（#275 放大）

删除路径只做「死卡打标 → 废弃 quiz 回合 → purge 8 张内容表 →
墓碑」：`service/course.rs:751-811`。在途生成任务不可远程取消
（`generation_tasks.rs:601-605`），全局清扫跳过课时任务（`:615-630`）；
课时草稿 TTL 1h 不清（`lesson_draft.rs:57-77`）；`apply_batch`、结构快照
UPDATE、挂载 upsert、课时发布全无存活复核（`learning_graph.rs:3938-4087`、
`:456-460`、`:885-901`；`lesson.rs:2239-2250`）；purge 清单不含
growth batches/mounts/findings/repairs/maintenance/system_actions
（`course.rs:912-942`）。本仓不会「目录复活拖垮列表」，但会留下墓碑课的
任务 pill 与迟到写污染。

### pg4 已学完仍可置跳过（#286 同构）

`progress.rs:54-145` 置位不查当前态；置 skipped 时把 `completed_at`
清掉（`:104-107`），XP 账本不删（`xp.rs:36-44`）。课程卡分子
`settled_or_completed` 不过滤 skipped（`course.rs:238-240`、`:1092-1097`），
分母剔 skipped（`model.ts:425-426`），进度条又用 `completed_lessons`
（`CourseCard.tsx:113-114` vs `:204-207`）——可读出「1/1 课时 + 0% 条」。
XP gauge 剔 skipped（`xp.rs:284-301`），quiz 触发计数不剔（`:428-446`）。

### pg5 归档课按课 quiz 面（#278 同构）

按课 `quiz_progress` 只筛 `deleted_at` 不筛 `enabled`
（`xp.rs:450-459` → `routes.rs:161-163`），归档课 due=true；`quiz_start`
只看 due 不看启用态（`quiz.rs:220-230`），直连可开考。聚合面已筛
（`xp.rs:492-495`，测试 `tests.rs:11830-11846`），UI 也藏了入口
（`LearningGraphWorkspace.tsx:173`），所以当前无症状，API 语义仍矛盾。

## 四、裁决

| 项 | 裁决 | 理由 |
| --- | --- | --- |
| Stage 0 测量 | **做**（pg0） | 本仓零读数；enforce 档与 Stage 2 都要它 |
| Stage 1 机械域 | **做**（pg1，分规则执法档） | 与 CONTEXT「登记表只作身份/命名」「已知集=已教∪声明」直接冲突；pg0 实测直出违规 63.2%，不能一把 enforce |
| Stage 1c 声明地板 | **做**（pg2） | 声明块已落盘未消费；机械减法 ~30 行，风险低 |
| 删课收尾 | **做**（pg3） | 墓碑课任务 pill + 迟到写污染；删旧课重建是常用流程 |
| 跳过/已学完互斥 | **做**（pg4） | 语义矛盾 + 读数分裂脑；存量读侧自愈 |
| 归档课按课 quiz 面 | **做**（pg5） | 三行查询 + 一个守卫；API 自洽 |
| Stage 2 derivation+账本 | **暂缓** | pg0 样本 64 次调用/0 翻转对；账本是新机制 + 新观测文件；等 live 档案积累后再判 |
| Stage 3 primitive/bootstrap | **不搬** | 本仓无链接器；从零已有空账地板；schema/UI 成本高、无本仓病灶 |
| 其余 9 票 | **不搬主线** | 6 票本仓已有/口径已裁；3 票小件（#277/#280/#282）见第七节 |

## 五、票

| 票 | 标题 | 依赖 | 状态 |
| --- | --- | --- | --- |
| [pg0](tickets/pg0-flip-surface-measurement.md) | 真机档案只读量翻转面（不改门） | — | closed（直出 63.2% / 在册 0.3% / 兄弟 7/9 / 翻转 0） |
| [pg1](tickets/pg1-known-set-two-faces.md) | 已知集两面拆开：在册≠已知、直接出处、自教同节点、声明值 | pg0 | closed（真机校准欠账） |
| [pg2](tickets/pg2-declared-floor-first-batch.md) | 首批参考面减去已会声明块 | pg1 | closed（真机两臂欠账） |
| [pg3](tickets/pg3-delete-inflight-cleanup.md) | 删课收尾：在途任务/课时草稿/迟到写 | — | closed |
| [pg4](tickets/pg4-skip-vs-learned-double-count.md) | 已学完仍可置跳过：账本/读数双计与卡片分裂 | — | closed（拆读欠账） |
| [pg5](tickets/pg5-archived-quiz-api-parity.md) | 归档课按课 quiz 读数/start 与聚合面口径对齐 | — | closed |

建议顺序：pg0 → pg1 → pg2；pg3/pg4/pg5 可并行（互不依赖），但同一
时期不要把 cargo 构建/测试并发跑满（AGENTS.md 的验证阶梯）。

### 影响面总表（一屏）

| 票 | 生产面 | 测试面 | DB/UI/迁移 | 主要风险 |
| --- | --- | --- | --- | --- |
| pg0 | 仅 `assets/` 一次性脚本 | 无（复跑一致） | 零/零/零 | 当前登记表 ≠ 当时；样本偏差 |
| pg1 | 5 文件 + smoke 读数 | `provenance.rs` ~35 测试 + schema/basis/读数 | 零/零/零 | 在册移出 known → 翻红面；命名池误删 |
| pg2 | 4 文件（terrain/CoachContext/声明读取） | `structure_view` 8 调用点 + `CoachContext` ~15 夹具 | 零/零/零 | 精确字符串；空集必须逐字节同旧 |
| pg3 | 8 文件（删除/任务/草稿/写路径/UI pill） | 删除族 5+ + 任务/生长/课时发布 | 零/可能 UI/零 | lesson 任务不可取消；审计表去留 |
| pg4 | 9 文件（progress/course/models/xp/UI） | progress/xp/course/model/card | 零/可能 UI+i18n/零 | 撤销完成通道；存量重叠行 |
| pg5 | 3 文件（xp/quiz/守卫） | quiz progress/overview/start | 零/零/零 | 归档 start 变 409 的行为收紧 |


## 六、风险与不变量

- **拆池不是删池**：`derive_naming_list` / 碎片掩码仍要一个「命名池」
  （登记表活跃名 + 本批名），否则登记表名字会全变未着陆片段。
- **rs2 归一同步拆**：归一是第二个用平集合的地方；只改校验不改归一会
  把「在册未教」重新洗回已教/自教。
- **审计面已窄**：挂号表本来只覆盖 mints/new_aliases 的定义
  （`mod.rs:1041`），intent/scope 走碎片表；本波不动。
- **复用既有定义的前置核**（评审判据 #2，`mod.rs:1112`）用已教账，不是
  平集合；不得因拆池删除或改口径。
- **执法档**：分规则——`已教=covered` enforce；直接出处/自教同节点先 report-only（pg0 实测直出 63.2%），提示词校准后再翻 enforce；纯零整表 report-only。不新增
  第二把开关。
- **声明地板只减自报事实**：不按结构依赖/深度裁未声明块，不级联
  （ADR-0015 分界）。
- **表完备性硬门不加**：碎片面缺省 report-only，加硬门会改已裁定默认；
  另票裁决。
- **pg3 不碰硬删除/恢复**：墓碑语义（ADR-0017）不动；迟到写防御是
  「可见失败、零写」，不是静默丢。
- **pg4 不碰账本删除**：XP 事件独立保留；只改置位门与读侧口径，存量
  重叠行读侧自愈。
- **pg5 不动复习卡/到期队列**：只补按课 quiz 面的启用态与 start 守卫。

## 七、顺手小件（未立票，等主线后裁）

- **redraft previous_json 仍 pretty**：`mod.rs:944` 明注「另票处理」；
  learnhub #250 的紧凑化可作参照（review 入参已紧凑，`mod.rs:1630-1633`）。
- **任务收尾一行汇总**：`learning graph growth batch accepted`
  （`service/learning_graph.rs:2104-2112`）已有 elapsed/attempts 但无
  llm_calls/tokens；`learning-tasks.jsonl` finish 行有 llm_calls 无 tokens
  （`generation_tasks.rs:555-575`）。learnhub #277 的「一行汇总」是薄层
  便利。
- **传统课生成线缺 pf4 写作条款**：agent 线
  （`lesson_content_loop.rs:152-196`）已有模型边界/例题收尾/逐项对应/
  可控差异/自足；传统线 `generation/lesson.rs` 只有类比边界（R17），
  其余零命中。属本仓内部一致性问题，与隔壁这波无关。
- **生长任务批计数**：`generation_tasks.rs:76-97` 有 elapsed/step/
  attempt；growth 只 set 一次 `draft`（`learning_graph.rs:2066-2068`），
  无「已装配 k/N」。learnhub #282 的形制可参照，但本仓 pill 没有逐节点
  重复句的问题。

## 八、Decisions so far

- **pg0**：真机 64 次评审 / 673 行——直出违规 63.2%、在册未教 0.3%、
  兄弟供给 7/9、翻转 0；Stage 2 账本不因本样本开票。
- **pg1**：三面已知域落地；`covered` enforce、直出/同节点 report-only
  （`stage1-enforce` 可一键全执法）；真机校准欠账；回放臂无法量
  `registry_only`（`from_known_names` 把 known 全放 covered），已在票面
  记录。
- **pg2**：首批参考面减声明块（slug 精确、不级联）；真首批 = 课时表空，
  不是花名册空（后者含「无就绪候选」的中途态）。
- **pg3**：删课取消可取消任务/清草稿与唤醒、purge 六张观测表；四条
  迟到写守卫；在跑 lesson 任务仍不可取消（发布守卫兜底）。
- **pg4**：已结算不可再跳过；读侧 settled 优先；UI 分子分母同口径；
  「已跳过 K」拆读另票。
- **pg5**：归档课 `due=false`、start 409；恢复后自然回来。
- **双轴评审（2026-11-27）**：修复读数里的本机绝对路径、草稿清点时机
  （purge 前）、Stage 1 档位可观察（review 日志 + smoke `stage1=`）、
  `pool` 收窄到登记表+本批、别名概念身份判定、`CourseLiveness` 枚举、
  skipped 片段/取消遍历单源；详见 pg1/pg3 Resolution。
