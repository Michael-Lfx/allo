# learnhub #99 产品波深挖（fb72157..HEAD，约 60 提交）

> 调研日期 2026-10-14；为票 [learnhub #99 产品波深挖](../tickets/r3-product-wave.md)
> 的结案资产。

## 1. 「现存且启用」口径（#102 核心裁决）

规则单源 `active_course_ids`（`domain/courses.rs:528-536`）：

```rust
/// 「现存且启用」课程 id 集（读侧横切口径单源）：课程目录在场（目录为真相）
/// ∧ 注册表 enabled（条目缺席 = 滞后合法，视为启用）。归档与已删课程不在此集
/// ——安排面（到期队列/教新候选/quiz 覆盖/进度尺）的过滤一律以此为准；
/// 历史面（xp 日历/账本摘要/掌握度聚合）不过此筛，账目流保留。
```

- **安排面全收窄**：到期队列（`scope_review_stream` 纯函数过滤）、复习中心、教练解析
  （点名归档课 = 「课程已归档，恢复后可继续学习」）、候选窗口、单课概况 due（归档强制 0）、
  XP 进度尺分子分母同筛、自由练习抽取。
- **历史面全不收**：今日 XP、xp 日历、账本摘要如实保留。
- **已删课程不 Broken**：删除 = 先目录后注册表；账目四流住空间根不随课删；逐课程历史行
  以课程目录为数据源，「删课即无行可列」而非标「已删除课程」（code-review 定案：如实承诺）。
- **注册表滞后补立** `mutate_registry_entry`：立课「先目录后注册表」的崩溃滞后是合法态，
  治理写面由课程目录补立条目再改。

## 2. 课程管理写面（#102）

- **rename**：只动注册表显示名；空白/不存在 = 人面错误；**宪章目标原句一个不动**（生成
  快照纪律）。显示名 = 人面治理事实（立课默认 = 目标原句，可改），与宪章 goal 两处都展示，
  后者悬浮提示。
- **archive**：注册表 `enabled` 翻转（false = 归档、置回 = 恢复）；**数据零删零改**，是
  读侧安排面的唯一排除依据。列表出参缺席条目回退（名 = 宪章目标原句、enabled = true）。
- **delete**：先目录后注册表；课程域产物随目录退场、账目流保留；目录删失败 = 整体报错
  零变化；注册表写失败 = 条目残留（读侧降级、下次删除幂等收尾）。
- **概况四读数**：`CourseOverview{node_count(未归档), settled_count, due_count(enabled
  才计), total_xp}`，`overview_inputs` 单次装载喂列表行与单课概况（零第二条读路径）。
- **删除强确认（#112 GUI）**：`canDelete = confirmation.trim() === course.name`（逐字输入
  显示名）+ 三条如实后果：「课程文件将永久移除…」「学习记录（账本）保留：已计入的进度与
  XP 不随之删除；此后各读面不再出现本课」「此操作不能撤销；只是想暂停这门课，请改用
  『归档』」。

## 3. 长任务注册表与协作取消（#103，`app/generation.rs` 1370 行）

- **状态机**：`running → done | failed | cancelled`；三终态互斥、**终态幂等**（首达为准，
  重复收尾不改写不重发）；同键并发 begin 拒绝（防双击双跑）；终态后可重新 begin。
- **注册表**：键 = `{kind}:{course_id}:{node_id|-}` 单源；四任务型 Grow | TeachPrep |
  QuizVariants | QuestionReplacement；进程内存态 + 观测面 JSONL 留痕（旁路非执法）；
  **注册序** = AtomicU64 进程级单调（同秒 ISO 不可作「最近」判据——真实缺陷由同秒并发
  测试暴露后收口）；快照排序「在跑置顶，其余注册序倒序」。
- **进度事件**：`GenerationTaskEvent{task, kind, course_id, node_id, status, step, detail}`
  ——step 机器面英文、detail 人面中文；事件发送者 `TaskEmitter = Arc<dyn Fn>` 抽象
  （业务层零宿主类型，测试注入收集器）。
- **取消哨兵 + LLM 前后双检查**：哨兵常量「【已取消】…」（全角括号防撞模型输出）；沿错误
  Display 链判别；`ReportingExecutor` 包装：complete/complete_json 均 enter_step（**前
  检查：拦截则内层零调用**）+ exit_step（**后检查：调途中取消则丢弃生成结果**）。关键裁决：
  旗标内存写不经引擎闸锁恒即时送达；**闸锁全程持有至操作自然退出，取消不弃闸、不中断
  在途请求**（「请求级中止需 async 改造波及全部管线，拒绝」——learnhub sync 接缝语境；
  allo 是 async 栈可真中断，见移植注意）。单步任务生成开始后才置位 = 竞速落败，终态如实
  done（诚实原则）；**取消 = 中性终态不报错误**，命令出参升 outcome envelope。
- **步骤叙事映射** `step_of(kind, llm_task)`：同任务名不同型叙事不同（question-generate
  在教新 = 「正在出题…」）；生长 = select → pack/validate → place；未知透传不 panic。
- **四类长操作接入**：生长 runner；教新备料 + quiz 变式（start_session 备料任务化、取消 =
  `StartOutcome{cancelled}`）；单题重出（调用方持引擎闸锁）。命令三件：grow_nodes /
  generation_tasks（快照）/ cancel_generation_task。
- **前端共享 hook**（`shared/lib/generation-task.ts`）：`generationTaskKey()` 与后端同形 +
  `useGenerationTaskDetail`（按型/课程/节点过滤，返回最近 detail——文案单源在后端，前端
  不造步骤文案）；**事件缺席不碍流程——命令 promise 兑现是完成态唯一真相**；收尾清叙事
  防闪现。三消费方：学习页「正在准备…」、题库重出进度条、课程页生长按钮。

## 4. coach_view（#111）

五卡（四结构卡 + 一前端折叠卡）：

1. **接下来** `next: PlanView{state(ready|resting|complete|empty), kind(teach|review|quiz),
   course/node, reason, est_minutes}`——与「今日安排」同形同源；理由 = 教练决策人面中文；
   深链「去学习」。
2. **检测进度** `quiz_progress[]`——「距上次检测 N/K 次学新结算」，K 单源常量。
3. **复习重点** `review_focus[]`——归因人面，只收需注意投影（「保持」不收）；排序 = 升级
   态 < 链断 < 流利；note 单源归因话术函数。
4. **学习节奏** `pace{debt, debt_limit, brake_on, gap_days, recovery, lines}`——护栏读数 +
   话术行单源，「只报安排不承诺记忆效果」。
5. **课程材料**：前端折叠卡（冷启动切片降级）。

**预览式解析**：`preview_next_scoped` 与 resolve **同一条队列序代码**（同一实现只换
ResolveFlavor::Preview），纯读两保证：零档案写入（不产生「从未执行」的决策档案）+ 零生长
零 LLM（`readonly` 引擎把执行器换成 PreviewOnlyExecutor——**任何意外 LLM 调用即报错的
结构性保险**、观测面关闸）；课程参数只收窄教新槽位（队列序唯一）；空态预筛（到期与候选
皆空不进预览，防页面打开静默烧额度）。

**禁语围栏**：

```rust
/// 禁语词表（零机制成本——围栏测试钉住，运行时零守卫机制）：
/// 教练对人面的一切理由/话术禁绝对化断言——掌握是证据账本上的读数，
/// 不是终审判决；措辞越界 = 缺陷（竞品复核纳入项）。
const BANNED_ABSOLUTES: [&str; 6] = ["完全掌握", "彻底", "永远", "百分百", "百分之百", "绝对"];
```

围栏测试遍历 resolve 全队列档位 + 护栏话术 + 归因结论 + 掌握度理由，「任一路径越界即红」；
变异验证（改话术含禁词 → 红）可拦。MCP 同源扩展（观测面 + guide 合并出参）。

## 5. 重出流程（#110）

替换语义：「生成一道替代题，替换语义 = **原题作废留档 + 新题追加——同一次原子写**」；
prompt 不带存储 answer/keywords（持答案红线，结构上不可能）；目标须未作废（「待修题正宜
重出——重出即处理」）；作答历史按题 ULID 留原题（审计连续）；原条目只盖 voided 态零改写。
七命令：list/detail/edit/set_edit_pending/void/restore/regenerate。读投影：列表行脱敏，
**单题详情 = 管理面揭示**（全字段下发，红线：绝不进学习者会话面）+ 作答历史。前端单步
替换确认 → 生成中进度条（消费 #103 + 取消）→ 结果条背 envelope（cancelled 显「已取消，
原题保持不变」）。

## 6. #109 / #113 / #104

- **复习中心**：`ReviewCenterView{due_groups(按节点分组), due_today_count, overdue_count,
  upcoming_7d_count, upcoming_days(恒 7 桶首桶 = 明日)}`；组序最逾期在前；收窄同 #102
  （「恢复后同一张卡照自然时间回队」）；负载计数取自到期投影本面（不做题面漂移过滤——
  负载是真实安排）。
- **#113**：唯一主 CTA；xp_calendar 84 格（零桶显式在场，「零与缺席是两回事」）；今日到期
  读数复用复习中心投影。
- **#104**：`quiz_history`（历史行序 = 完成时间新→旧平局 ULID 逆序；容错分级——清单页坏档
  跳过留痕、单次详情点名读档 fail loud）；校准读数（预测 R vs 实际）进结算卡与历史行。

## 7. 契约 v2.6 R9 + 跨课预告废除

> **跨课预告废除**：小结收在本课以内；根因：跨课意图无兑现通道（兄弟课生成只读得到课名+
> 概念，读不到本课正文；实测「接下来」承诺零回溯），**无通道保证的承诺不发**。

> **R9 叙述声部**：正文对学习者说话：自称「本课/这一课」，兄弟课称「上一课/下一课」，「节N」
> 只指本文内的步骤节；开发面结构词（本点/学习点/节点/挂接/投影/骨架）不进正文与节标题。
> （真机抽查实证：「本点」「下一节」漏入正文。）

## 8. allo 对照表

| learnhub 能力 | allo 现状 | 差距 |
|---|---|---|
| 课程删除（目录退场 + 账目保留） | 软删墓碑 deleted_at（069）+ deleteReviews 可选连带删卡 | 「现存且启用」读侧横切口径可平移；强确认文案须按 allo 实际语义改写（若保留连带删卡不得虚诺） |
| 生长进度事件 | WS 两流（phase 判别）+ 批行状态机（068） | 不重放、无统一任务注册表快照命令 |
| 生长取消 | `generation_registry{run, cancel: AtomicBool}` + cancel 端点 | **假取消实锤：growth_body 循环零检查点，取消后跑完并落库**（仅 Err 分支改标 cancelled） |
| 日历读数 | calendar_stats（打卡事件计数、review-day 翻转） | 换 XP 桶 84 格形态（二轮 g1 已裁） |
| 复习队列 | due_reviews 直查 SQL（遗忘风险排序） | 无聚合读命令（分组/分桶/徽标一次取齐） |
| 题目编辑 | update_question 全字段 + edit_pending（044） | 无作废留档/恢复/重出/作答历史抽屉 |
| 删除确认 | CourseDeleteDialog + deleteReviews 勾选 | 无输入显示名强确认 + 如实后果三条 |
| 课程概况 | CourseSummary{total/completed/enrolled} | 无今日到期/累计 XP/归档态四读数 |
| 改名/课程级归档 | **无**（无 PUT/PATCH；归档只有复习项/自定义题级） | 净增量 |

## 9. 移植优先级与三个注意点

1. **P0 现存且启用 + 改名/归档/删除强确认**（裁决自洽风险最低；allo 补 enabled 开关 +
   active 集单源收束安排面查询，历史面明确不过筛）。
2. **P0 长任务注册表与真取消**（allo 假取消是行为缺陷非缺功能；状态机/终态幂等/中性终态
   envelope/哨兵判别/叙事映射/注册序判据可整体照搬；async 栈用 CancellationToken 在
   await 点前后插双检查重写双检查器）。
3. **P1 禁语围栏（零成本纯测试先行）+ coach_view 读面**（可先移植读侧投影形态，预览式
   解析等调度面成型后接）。
4. **P1 复习中心聚合读数**。
5. **P2 题目维护页与重出**（依赖任务化）；**P2 总览入口/检测读面**（检测依赖 quiz 会话
   模型）。
6. **随批契约 R9/跨课预告废除**（写入生成提示词与契约文档，纯文本）。

注意点：①强确认文案承诺「学习记录保留」——allo 若保留 deleteReviews 连带删卡，文案须
改写（不得虚诺）；②learnhub 取消不中断在途 LLM（sync 接缝），allo async 可真中断，但须
定义「中断后已完成部分保留」的幂等落库语义（learnhub 靠批内原子写 + 幂等步进兜底）；
③快照排序用进程级注册序而非时间戳（同秒并列缺陷），allo 批行已有 DB seq（068）照搬。
