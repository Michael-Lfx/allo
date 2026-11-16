# Map: 建课进度呈现收口——学习图弹窗分裂态、恢复态不收敛、后端文案 i18n（c1–c3）

<!-- wayfinder:map -->
<!-- 状态：立图（2026-11-10）。来源 = d5（建课流程简化）合并后首次真机学习图
      建课实测（目标「通盘认识股票市场基础」，课程 01a1c440-cfba-72e2-896b-
      e728dcd4fc96）：提交后弹窗步骤条永久冻结在「正在生成知识结构与前置地
      形…」，而后端结构生成 139.7s 即 done（60 块 / 7 门 / 11 标注）。本会话
      逐文件核实呈现链路、恢复链路、文案泄漏面后立票，c1 呈现策略经用户裁决。 -->
<!-- 基线：feat/learn0928 尖端 498773e15（d5 并回后）。 -->
<!-- 状态更新：收图（2026-11-10）。c1–c3 施工完毕 + 双轴代码评审（Standards /
      Spec）+ 评审修复一轮后闭票；集成 = feat/learning-create-progress-ux，
      并回前已合入 feat/learn0928 新尖端 a2f0d288c（d6–d7 在飞恢复）并在合并
      树上重跑闸门。真机判据（三票目验项）全部未执行，留待用户实测，FAIL 按
      票重开。收图记录与后置清单见文末。 -->

## 用户裁决（2026-11-10）

1. **c1 呈现策略 = 最小修**：学习图流不再渲染 v2 步骤条/活动流——running 时单行
   「结构生成已转入后台」spinner，completed 后只留课程卡片；真实结构进度在工作区
   查看（structure 徽章 + e3 声明窗口已在）。备选「对话框内学习图专用步骤条」
   （与工作区重复建设 + WS best-effort 丢帧无兜底）与「提交即跳工作区」（失去建成
   确认时刻）均否决。
2. **c3 范围收窄 = 只动学习模块**（前端 `pages/learning/` + `nomifun-learning`
   内注释）：后端 WS `text` 帧**原样保留**——宠物气泡（companion 侧）是 text 的
   在网消费面，删帧即跨模块行为变化，本波不碰。已知残留：en-US 下宠物气泡
   detail 行可能显示中文 learning 文案，归属 companion 后置（现状保持，非本波
   回归）。

## 发现 → 三票映射

| 发现 | 分类 | 归票 |
| --- | --- | --- |
| ① 学习图建课 HTTP 秒回 → 弹窗 `completed` 卡片与冻结的 v2 步骤条/起始帧文案同屏分裂，且永不更新 | 用户可见缺陷（d5 收尾遗漏） | **c1** |
| ② 页面切换后 `refreshGenerationStatus` 重建的 running/queued 态无任何机制推到终态——结构 15:01 已完成，指示条永远「生成中」 | 状态机缺陷（v2 恢复态同病） | **c2** |
| ③ 后端 9 处 `set_step` 中文 detail + 3 处 topic 中文前缀直达学习模块 UI，en-US 用户看中文（`generation_tasks.rs:84` 注释明写「detail 人面 = 后端单源单句中文」，系成文契约需推翻）；WS `text` 帧的在网消费面含 companion 宠物气泡，帧保留不动（裁决 2） | i18n 红线违规（学习模块内收口） | **c3** |

## 影响面核查关键事实（2026-11-10 实码核实，票面直接引用）

- **学习图建课 HTTP 秒回**：`learning_graph.rs:129-193`——INSERT 课程 + 1 模块
  占位 → `admit_structure_generation` 拿锁 → `tokio::spawn` 分离结构生成 →
  `course_detail` 即回。首生长不衔接（e3：`:189-191` 注释明写「首批 = 用户显式
  POST grow」）。真机：14:59:44 提交，卡片即现；结构 15:00:44 begin →
  15:01:23 done（learning-tasks.jsonl + nomicore.log `knowledge structure
  snapshot stored`）。
- **弹窗 WS 订阅只在 running 时挂着**：`CourseGenerationProgress.tsx:62-67`
  `if (status !== 'running') return`——graph 流 status 数秒翻 completed 后订阅
  停用，`structure_completed` 帧永远进不来（也进不来：该帧带 `course_id`/
  `event` 字段，真消费者是 `LearningGraphWorkspace.tsx:176-177`）。
- **步骤条推导只认 v2 管线帧**：`model.ts:122-146` `deriveStep` 只映射
  round/audit/publishing/completed phase；graph 流只有 started 帧入列 →
  step 恒 1；`CreateCourseDialog.tsx:109-117` 不传 `pipeline` prop，graph 流
  无 triage 帧 → `isModernPipeline` 恒 false → 恒走 legacy 五段步骤条。
- **graph 事件与对话框事件同通道异形状**：`emit_graph_event`
  （`learning_graph.rs:5018-5025`）加 `course_id/kind/event` 字段后走
  `emit_course_event` → `events.rs:119` `learning.course-generation`——与
  `ipcBridge.ts:1870` 对话框订阅同一通道；`useGraphGenerationEvents.ts:20`
  注释明说「建课对话框…形状不同，亦不经此 hook」。
- **恢复态无收敛路径**：`index.tsx:157-160` 仅挂载时执行一次
  `refreshGenerationStatus`；`useCourseCreation.ts:175-195` 从
  generationTasks 快照重建 running/queued 后，唯一能写终态的
  `generateCourse`（HTTP 回归）已不在飞行中——WS 帧不改 status，无轮询。
  `GenerationTaskView` 带 `course_id`（`generation_tasks.rs:77`）与终态
  state（`:80`），收敛后可直接接课程详情与「进入课程」。
- **文案泄漏面全量**（实码扫描，无扩散）：
  - WS `text` 帧 4 处（全在 learning_graph.rs）：`:295` structure_started
    「正在生成知识结构与前置地形…」、`:1867` growth_started「教练正在阅读
    结构参考与花名册，起草本批节点…」、`:5901` basis_declared「已会声明
    完成…」、`:6028` basis_revoked「已会声明已撤销…」。消费面：对话框
    活动流 started 行（`CourseGenerationProgress.tsx:164` 原样渲染
    `event.text`；c1 裁决后学习图流整段隐藏，v2 started 帧无 text → 该面
    实际已闭合，c3 仅留一行防御性加固；**订正（收图时）**：v2 round 帧带
    模型叙述 text（`crates/backend/nomifun-ai-agent/src/learning_loop.rs:
    292-299`，立票时扫描只覆盖 nomifun-learning 漏了桥接 crate），属动态
    模型输出非后端文案，round 行渲染保留）+ **宠物气泡（`companion/messages/
    project.ts:117,134` 原样渲染 `evt.text`）= 唯一跨模块在网消费面，帧
    保留不动（裁决 2），残留归 companion 后置**。
  - `set_step` 中文 detail 9 处：`generate.rs:391/439/508`（v2 三档）、
    `learning_graph.rs:289/546/873/1851`（structure/mount/repair/growth
    draft）、`lesson.rs:692/770`。消费面：生长 Pill 裸渲染 detail——只有
    `draft` 档有 i18n key（`GrowthTaskPill.tsx:47-48`
    `if (item.step === 'draft') return t('learning.growthPillStepDraft');
    return item.detail;`），key 单语先例在
    `zh-CN/learning.json:83`。
  - topic 中文前缀 3 处：`learning_graph.rs:236`（「知识结构生成」）、
    `:5011`（「学习图生长 · {title}」）、`lesson.rs:612/636`（「课时内容 ·
    {title}」）。消费面：恢复态悬浮 Pill 的 `topic` prop
    （`index.tsx:448-450` = `request.description` = 恢复时写入的后端
    topic，`useCourseCreation.ts:187`）；生长 Pill 走
    `growthCourseTitle` 剥前缀（`growthTaskPill.ts:156-158`）。
- **成文契约需推翻**：`generation_tasks.rs:82-85` 注释明写「step 机器面
  （英文常量；前端不造步骤文案）/ detail 人面（后端单源的单句中文）」——
  c3 是设计决定变更（前端造人面文案，后端只留机器面），非顺手改硬编码；
  DTO 注释与相关测试断言同改。
- **陈旧注释（随 c1 顺带订正）**：`api.ts:55-57`、`CreateCourseDialog.tsx:14-17`、
  `useCourseCreation.ts:155-156` 三处仍写「首生长在后台启动/执行」——e3 已删
  首生长衔接。

## 施工顺序建议

c1 与 c3 同触 `CourseGenerationProgress.tsx`（不同面：呈现结构 vs started 行
文案），建议 c1 → c3 减少冲突；c2 独立随时可做。三票无硬阻塞边。

## 非目标（全波）

- **学习模块之外的任何改动**（裁决 2）：companion 宠物气泡、其余 WS 消费方
  ——后端 `text` 帧原样保留，跨模块残留明记于上、归属 companion 后置。
- AppError 错误文案 i18n（error-surface contract 管辖）。
- observation `learning-tasks.jsonl` 诊断日志文案（诊断面，保留中文）。
- 对话框内 structure_* 订阅 / 自动跳工作区（c1 裁决否决项）。
- WS 事件名与 phase 常量（机器面，双语无关）。

## 收图记录（2026-11-10）

- 施工：c1 = `1c4e951ea` + `02402fa55`（票外回归修复）；c2 = `e733cac5e`；
  c3 = `37db88047` + `454775748`。单票一代理并行（worktree 各自分支），merger
  依次并回集成支；tracker 随 `b5e364d5b` 入 git。
- 双轴代码评审（Standards/Spec 两子代理并行）→ 修复一轮 `10b2cb636` +
  `2619cc2e0` + `a8bb2700a`（F1–F8）。评审要点：恢复态重试危险路径（Spec 最
  重）、双语断言缺口（Standards 硬违规）、round text 前提订正（立票扫描盲区：
  nomifun-ai-agent 桥接 crate 未在扫描范围）。
- 并回前合流：feat/learn0928 新尖端 `a2f0d288c`（d6–d7 在飞恢复）先并入集成
  支（`56d28dae2`，文本零冲突），合并树重跑闸门全绿——learning 156 测试、
  i18n 11605 keys、typecheck 54 存量零新增、`cargo check -p nomifun-learning`
  过。d7 的恢复过滤收窄（只认 `outline`/`structure`）与本波 c2 语义互补。
- 评审留痕不修（judgement call，本波裁决记录）：集成线 merge 提交沿学习线惯例
  保留（`8f8dfdbdf` 为携带生产代码的 evil merge，hunk 已在评审中显式核对）；
  `STEP_KEYS` 七档不可达防御映射保留；第三处手写轮询未抽公共；`kind`/`state`
  裸串与 `index.tsx` prop 下钻未重构。
- CHANGELOG：Unreleased 三条（弹窗分裂态、恢复态终态收敛、文案本地化）。

## 后置清单（收图时明记，均非本波回归）

1. **v2 outline 任务注册表 `course_id` = 会话 id**（`generate.rs:129-134`）：
   恢复的 v2 任务翻 done 落地 failed+原因而课程实际建成；修复需注册表契约
   变更，待在飞恢复波尘埃落定后另立。
2. **companion 宠物气泡 en-US 中文残留**（`companion/messages/project.ts:117,
   134` 渲染 `evt.text`）：裁决 2 划出范围，归 companion 侧。
3. AppError 错误文案 i18n（error-surface contract 管辖）。
