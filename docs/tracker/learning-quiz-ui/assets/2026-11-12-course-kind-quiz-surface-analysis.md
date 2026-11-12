# 分析：课程形态 × quiz 面——三轴缺口全景（2026-11-12）

> 三路只读勘察交叉汇总（形态×面矩阵 / quiz 形态耦合 / 造数资产）。本文档只做
> 分析与裁决点整理，不含施工裁决；立票以本文档为事实源。
> 触发：真机走查暴露「造数节点未开始却已 quiz/复习」「quiz 入口挂在传统工作区
> 」「检测到期零感知」三个问题，勘察后定性为入口轴/感知轴/行动轴三类缺口。

## TL;DR

- quiz **后端完全形态无关**（全链零 course_kind 引用）；图课程学节点走同一条
  进度→结算→种卡链，**会自然积累 quiz due，但无处开测**。缺口定性 =
  **「入口没挂」而非「语义不适配」**；历史波票无「图课程后置」明文裁决，
  属架构惯性。
- **感知轴是最大产品缺口**：检测到期零主动呈现（首页/图工作区/通知全无），
  learnhub 把 quiz 放在「今日安排」与教新/复习同列——日程语义 vs 仓库工具。
- 软闸（R 建议条）发现**三处实现与文档/注释不符**（含一处读数口径偏差）。
- 图课程可零 LLM 纯 SQL 造（最小 5 表，seed v3 约 +150–250 行），真机验收
  无造数障碍。

## 一、形态基本面（勘察实锤）

- `course_kind` 仅两值：`traditional`（缺省）/ `learning_graph`
  （models.rs:1937-1964）。图创建只走描述流（learning_graph.rs:138 硬编码，
  带知识库 400）；Agent 工具只能建传统课。
- 前端分流唯一开关：`detail.graph` 仅当 `course_kind == learning_graph`
  才组装（course.rs:421-429）→ index.tsx:334 分支。
- 旧 DAG 图已在 070 退役；现模型 = 概念网 + 生长（无先修边，就绪集读时
  计算，无表——learning_graph.rs:3987）。
- **学习闭环形态无关**：`update_lesson_progress` 无分支（progress.rs:30），
  完成即种卡（:103-117）+ 结算 node-completed（:121）+ 生长钩子（图多出，
  :125-127）。前端两工作区用同一组处理函数与同一 `LessonBlock`。

## 二、形态 × 面 矩阵

| 面 | 传统 | 图 | 备注 |
|---|---|---|---|
| 推荐入口 | 有（单课时推荐卡） | 有（就绪集前 10） | |
| 大纲与任意课时回看 | 有 | **无**（只能选推荐节点） | 被软闸挡下/已完成节点 UI 打不开 |
| 课时学习/作答/完成 | 有 | 有 | 同组件同 API |
| 跳过节点 | **无**（后端支持） | 有 | |
| 种卡 + node-completed 结算 | 有 | 有 | 同一条链 |
| **知识检测 quiz** | **有** | **无** | **后端可用，图缺入口** |
| 课程内到期读数 | 有 | **无** | DTO 有字段，图未渲染 |
| R 软闸建议条 | 无 | 部分 | 纯文字 Alert，无跳转 |
| 学习记录/系统动作/概念表/结构图 | 无 | 有 | 图专属 |
| 已会声明/生长控制 | 无 | 有 | 图专属 |
| 来源披露/覆盖卡 | 有 | 无 | 图不接知识库 |
| 复习横幅 + 今日队列 | 有 | 有 | 列表页全局，形态无关 |
| 打卡/XP 读数/记忆健康 | 有 | 有 | 列表页全局 |
| 问题管理 | 有 | 有 | 列表页全局 |
| Agent 工具建课 | 有 | 无 | 仅传统 |

## 三、三轴缺口清单

### 轴 1：入口轴（quiz 挂在哪）

- **G1 图工作区无 quiz 入口**：后端全链形态无关（quiz.rs/settle.rs/xp.rs/
  routes.rs 零 course_kind）；图课程会自然累计 due 只是无处开测。挂点候选：
  就绪集推荐区旁 / 学习记录页签 / 课程头部——与「检测对象 = 已结算节点
  （回向）」的语境要对齐（图工作区推荐轴 = 未学节点（前向），两方向并存
  需视觉/文案区分）。
- **G2 传统工作区的入口语境**：现 QuizEntryCard 独立卡在推荐卡之后（u2
  裁决），形态可保留；但「检测对象弱先序」与「推荐卡前向序」并存时的用户
  认知未验证。
- 裁决点：图工作区挂不挂、挂哪个位置、入口文案如何区分前向/回向。

### 轴 2：感知轴（用户怎么知道该测了）——当前最大缺口

- **G3 检测到期零主动呈现**：第 6 个节点结算那一刻，全应用无任何信号
  （首页无横幅、图工作区无读数、无通知）。用户不点按钮 = 功能不存在。
- learnhub 对照：quiz 在「今日安排」预览与教新/复习同列（sessions.rs:793，
  kind=quiz）——**日程语义**（系统排程推到面前）vs allo 的仓库工具语义。
- allo 可承载面：①学习首页聚合横幅（ReviewBanner 先例；需新后端端点：
  每课程 N/K 列表——map 后置区已挂名）；②浮动坞（learningFloatingDock，
  未深勘，承载内容待查）；③nomifun-notify 推送；④「今日安排」面 allo
  **不存在**，新建是大工程。
- 裁决点：感知面选型（横幅/浮动坞/通知/组合）+ 时机语义（积累制呈现为
  计划制是否要做）。

### 轴 3：行动轴（测出弱之后发生什么）

- **G4 题卡级反哺（g4，原语齐缺链路）**：建卡 `ensure_review_item` /
  推进 `update_mastery_and_review` / 落库加题 `create_lesson_activity`
  三原语俱在（q3 §7 在案，**语义是题卡粒度**）。待裁决三设计点：
  rating_source='quiz' 进不进统计口径（自反放大器问题）、quiz 推进零 XP
  与否、变式落库节流。
- **G5 节点级补习：判修正**——非「整套缺」：图课程有 R 软闸建议条
  （供给节点已遗忘 → 建议先复习，ADR-0009 Amd1），但纯文字无行动入口；
  learnhub 有完整补救链（attribution.rs 归因 + deepen.rs 补救路由）。
  准确表述：**allo 有「发现」（软闸/弱先/校准），缺「行动」**（跳转、
  自动排程、重教流程）。
- 裁决点：g4 立票时机；软闸建议条要不要加行动入口（与 G1 感知轴合流）。

### 附：软闸三处实现与文档不符（勘察顺带发现，疑似小缺陷）

1. ADR-0009 写「提示条**直达复习**」，实现是纯文字 Alert 无跳转
   （LearningGraphWorkspace.tsx:738-749 vs ADR :84-87）。
2. models.rs:2216-2217 注释说被挡节点「仍可从学习记录进入」，实际学习
   记录行无点击，界面上进不去。
3. `ready_count` 读数是**软闸后**的数（learning_graph.rs:4006-4010），
   生长水位用**软闸前**的数（:3906-3912）——「就绪 x/7」读数偏低。

### 附：其它形态不对称（低优先级备查）

- 跳过节点后端两形态都支持，传统 UI 没有入口。
- Agent 工具只能建传统课。
- 图工作区 `index.tsx:332`「节点抽屉」注释过时（无抽屉，可选中仅推荐节点）。

## 四、造数资产（真机验收可行性）

- **图课程最小 INSERT 集 = 5 表**：`learning_courses`（course_kind=
  'learning_graph' + learning_goal + structure_json 全量信封，样例
  learning_smoke.rs:3387 `valid_structure_seed_envelope`）→ 1 隐含模块 →
  节点（lessons，带 est/content_generated）→ `learning_concept_registry`
  → `learning_lesson_concepts`（teaches/assumes = 先修与概念挂接，就绪集
  自动成立）。可选：`learning_basis_declarations`（声明面）、
  `learning_growth_batches`（历史页）。就绪集/软闸/水位读时计算无需造表。
- **seed v3 预计 +150–250 行零 LLM**；seed v2 的 progress/xp/cards/checkins
  四段对图课程逐字适用。测试先例：tests.rs:3646 `insert_graph_course_shell`
  系列、basis_tests.rs。
- 场景×造数：传统复习/传统 quiz/日历打卡 → 现 seed；图 quiz → seed v3
  （**但 UI 入口缺，只能 API/smoke 打，除非先补 G1**）；软闸 → seed v3
  + 供给节点低 R 卡；结构生成/挂载/修补/生长/声明 → 仅 learning-smoke
  （须凭据）。

## 五、待裁决清单（立票前逐条过）

1. 图工作区挂不挂 quiz 入口（G1）——挂的话位置与前后向语境文案。
2. 感知面选型（G3）——首页聚合横幅（需后端端点）/浮动坞/通知/组合；
   要不要向「今日安排」语义走。
3. 软闸三处不符（附）——立小票修正还是并入 G1/G3 的票一起做。
4. g4 题卡反哺（G4）三设计点 + 立票时机。
5. 节点级行动（G5）——软闸加跳转是不是第一步（成本最低的「行动」）。
6. 造数 seed v3（图课程）做不做、何时做（依赖 1/2 的裁决）。

## 证据索引（关键 file:line）

- 形态枚举/校验：models.rs:1937-1964、course.rs:949-952、070:797
- 分流：course.rs:421-429、index.tsx:334-370
- 学习链形态无关：progress.rs:30/103-121/125-127、learning_graph.rs:3663-3667
- quiz 后端零形态触碰：xp.rs:405-425/429-457、quiz.rs:184-206（仅 :1154/:1340
  借 defnear.stem_similarity 工具）
- 弱先 vs 软闸同源两投影：quiz.rs:739-850 vs learning_graph.rs:4028-4222
  （R_GATE=0.85 :4033）；ADR-0009 :84-87
- learnhub：sessions.rs:793（今日安排 kind=quiz）、attribution.rs:84 +
  deepen.rs（补救链）、统一 runner learn-page.tsx:34
- 软闸三处不符：LearningGraphWorkspace.tsx:738-749、models.rs:2216-2217、
  learning_graph.rs:4006-4010 vs 3906-3912
- 图课程造数：learning_smoke.rs:3920-3985（basis）、:3387（structure 信封）、
  :2589-2780（概念网 content）；tests.rs:3646（insert_graph_course_shell）
- seed v2（传统，已补 progress 行）：scripts/seed-learning-triggers.mjs
