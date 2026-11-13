# learnhub 会话与账本体系深挖（#87 线，fb72157..HEAD）

> 调研日期 2026-10-14；为票 [learnhub 会话与账本体系深挖](../tickets/r2-sessions-ledger.md)
> 的结案资产。

## 1. 题级卡下沉（#100 + 契约 v2.5 + persistence v1.17 + ADR-0022）

- 卡键三元组 `(course, node, question)` 全 ULID；`enroll_card` 幂等（建卡 = 一次性事实，
  重复 fail loud）；评级 = 会话作答结局（correct → Good，答错 → Again）。
- `advance_card`：同学习日重复推进 = `Folded`（**不落新事件，日粒度折叠**）；跨日落
  review 事件；未建卡先推进 = fail loud。
- `due_queue`：纯读投影，从 review-log 回放重建，排序 = (due 升序, course, node,
  question)——最逾期最先；跨课程只发生在扁平排序层。
- **persistence v1.17 事件族**：review 事件加 `question` 锚点 + **推进前快照四字段**
  （elapsed_days/stability_before/difficulty_before/r_pred——「永远落盘、无则 null 不省略」；
  「事后不可复算——参数重训会污染回放」）；practice.answer 加 elapsed_ms/response/
  session/forgot/predicted；journal.review-completed 加 origin(queue|practice)；learner-state
  事件化；会话记录新存储面。
- **ADR-0022**：「检索练习的粒度是单题；失败隔离只有题级能做到」；「与 allo 验证过的路径
  同构（迁移 039）」；否决混合方案（同一证据两处计数）与旧数据迁移。
- **allo 已有等价物（重要）**：复习卡已是题级且更早（`due_reviews` 注释明言每题一卡）；
  allo 缺的是：推进前快照四字段、同日折叠显式语义、同题幂等键、R 预测读数。

## 2. XP 三线入账（#88）+ 两把尺

### 2.1 定价（契约 v2.5 §3 + `engine/questions/settle.rs` ~340 行纯函数）

| 会话型 | 基数额 | 事件 | 授予 |
|---|---|---|---|
| 教新 | est_minutes（1 est 分钟 = 1 XP） | node-completed | 分级授予 |
| 复习 | 工作量当量 = 完成题数 × 题型当量（选择 1/短答 2/开放 3 分钟），**下限 3** | review-completed | 分级授予同律；幂等键 = 同课程同节点同学习日一笔；带 origin |
| Quiz | 同当量 | quiz-completed | **准确率分级恒非负**（0.9 足额+bonus / 0.6 足额 / 0.3 半额 / 四分之一额） |

分级授予 `award()`（教新/复习共用单源）：全对 = 基数 + xp_bonus(2)；部分 =
`(base × ratio).floor()`；砍短 = 0；**乱猜惩罚**：`n ≥ 4 && actual < 0.5 × baseline` →
`−max(base÷2, 1)`（基线：选择 = 1/选项数；文本 = 0.25 保守上界「宁可漏判不可误伤」）。

**写侧条款 `validate_settlement`**：full ⇒ 非负且 ≥ bonus 且无 ratio；partial ⇒ ratio ∈
[0,1] 必带无 bonus；trimmed ⇒ 恰 0；guess-penalty ⇒ 必为负；**quiz 专属：恒非负无惩罚
（测量非门禁）**。「数值由调用方算好，落账不计算」。

### 2.2 每日读数（`domain/xp.rs`）

零新状态文件，全部回放派生。`XpReading{today_xp, goal, goal_achieved, settled_nodes,
total_nodes, remaining_est_minutes, eta_days}`；**两把尺**：进度尺 = 结算数（只数
node-completed 去重），XP 尺 = 努力账（三线净和）——同源回放、互不换算；eta 绝不回写
决策面。历史面规则：日历**不过课程筛**（删课 XP 保留），进度尺分子分母都按现存且启用
收窄（「尺永不倒挂」有测试）。出参 i32（specta BigInt 禁令）。

### 2.3 每日目标

`DEFAULT_DAILY_XP_GOAL = 40`（≈ MA 课堂基线）、范围 1..=1000 越界 fail、住应用设置。

## 3. 会话体系（#105/#106/#107）

### 3.1 SessionRecord（`domain/sessions.rs`）

```rust
pub session: String,        # 会话 ULID（quiz = quiz 号）
pub kind: Teach | Review,   # review_form: drill|reference|relearn（teach = None）
pub origin: Queue | Practice
pub node / node_name        # 自由练习 node 空、node_name=「自由练习」
pub started_at / finished_at
pub questions: Option<Vec<{question, result, elapsed_ms}>>   # completed 时在场
pub answered: Option<usize> # abandoned 只带题数
pub earned_xp: i64, pub basis, pub terminal: Completed | Abandoned
```

纪律：**首写为终**（同会话号重写保持首写）、不携判分底、不触账本与调度；abandon =
「有头无尾也是数据」（零判分零落账零结算零推卡）。

### 3.2 学习者状态事件化

`PracticeEvent::LearnerState`（evidence_level 4、concepts 恒空「节点级自认不并入掌握
证据」）；**真实变更先落事件后写状态文件**；幂等空转零事件；「撤销不再零痕迹」。

### 3.3 五命令（+2）

- `start_session`：教练 resolve → prepare（教新/quiz 注册生成任务可取消，取消 =
  cancelled 中性态）→ **脱敏投影**入会话簿；`StartOutcome{state: started|resting|
  complete|cancelled, session?}`。
- `swap_window`（换一项候选窗口）：依赖序队首同批至多 3 项不跨批 + est 披露，纯读零 LLM。
- `grade_answer`：单题即时判分**纯函数不落账**；forgot = 跳判分直接揭示；
  `GradeFeedback` 是唯一解析下发口。
- `complete_session`（整卷结算）：逐题判分落 practice → XP 结算 → 题卡推进/建卡 → 会话
  记录；partial = 复习/练习提前收杆（教新/quiz 拒绝）；「成功出簿；失败留簿（账本步幂等，
  重试安全）」。
- `abandon_session`；外加 `start_practice_session`（自由练习：题数缺省 10 clamp 1..50、
  按预测 R 升序抽题、结算 = review-completed origin=practice「提前练」语义）与
  `coach_plan`（今日安排纯解析预览，空态预筛防页面打开触发生长）。

**脱敏投影**：字段级缺席 answer/keywords/explanation（强于占位）；判分底住服务端
会话簿（闸锁并发保护；判分纯读不进闸）；契约测试钉「无判底泄漏」。

### 3.4 统一会话页（#107）

三型同页（teach/review/quiz + practice 旗标）；阶段机 probe → content（逐节步进）→
reference（开卷）→ questions（逐题 + 双态反馈）→ 结算卡（读数 + node_next_due「下次
复习约 N 天」）；「想不起来」5 秒回忆门；标记卡点双入口；review 连对 ≥2 出「提前收杆」；
「结束本节」= abandon（「重来不落半账」）。

## 4. 渲染管线（#108）

`splitContentSegments` 切 `:::图` 占位 → 逐段 react-markdown + remarkGfm + remarkMath +
rehypeKatex；**RenderBoundary 渲染失败降级原文（fail-visible 不静默丢不 crash）**；围栏
路由：mermaid/echarts/函数 → 各 Block；html|svg|xml → SpecErrorCard（「自由绘制/脚本
通道不归内容」）；L2c 函数图像 = 手写递归下降求值器（**绝不 eval**，黑名单正则是安慰剂）；
每层硬标注「模型产物，非权威」；三处共用（会话页逐节/开卷参照/节点详情）。

## 5. xp_calendar（#113）

`calendar(space_root, weeks, now)`：weeks==0 拒绝；单趟回放 BTreeMap 按日聚合（负账如实
出桶）→ 恒 N×7 桶、末桶 = 今日、**零桶显式在场**（「空日留空不填 0」诚实度纪律落 UI）、
窗外不入、时间戳解析失败不计入任何一日。`XP_CALENDAR_WEEKS = 12`（恒 84 格）。
总览入口：「开始今天的学习」唯一主 CTA（纯导航深链，总览不预取会话）；「今日到期 N 题
另逾期 M 题」复用复习中心投影「不造第二把刀」；「非 streak 不做激励」。

## 6. 移植优先级（对照 allo）

**P0（二轮已裁、有完整可抄实现）**：①settle.rs 纯函数包（allo 九题型需扩当量表；AI 批改
轨是否入账需先裁）；②账本事件 + validate_settlement 写侧条款 + quiz 恒非负；③xp.rs 读数
投影（两把尺/ETA/84 格日历——对接「打卡并入 XP」换数据源）。
**P1（高价值无等价物）**：④推进前快照四字段（成本极低）；⑤会话记录面 + 首写为终 +
abandoned；⑥同日折叠显式语义 + 同题幂等；⑦xp_reading 现存且启用口径。
**P2（依赖会话体系先行）**：会话五命令/脱敏投影/教练解析、quiz 变式换一项、渲染管线
（markdown+KaTeX+fail-visible 可独立先行）。
**allo 已有无需移植**：题级卡本体、跨课程扁平队列、到期清单、自建题即卡、日历壳、
打卡日目标（换口径）。
