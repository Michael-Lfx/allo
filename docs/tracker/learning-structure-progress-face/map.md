# Map: 结构生成真话面——工作区徽章实时读数、Pill 不再撒谎、attempt 机器面（v1–v4）

<!-- wayfinder:map -->
<!-- 状态：立图（2026-11-11）。来源 = c1–c3 波收口后的首次真机实测复核：
      课程 01a1c8c3（2026-11-11 12:00:41–12:13:12，751s / 5 LLM 调用 / 1 次
      契约门拒收 → attempt 2 成功落库 41 块 7 门 16 标注）期间用户判定
      「流程的提示不足……对用户的误导反而更强烈」。本会话三层模拟定案：
      代码级 UX 走查 + 真实时间线渲染探针（真组件 + 真状态，S1–S5 输出）
      + 注册表/日志数据面盘点。设计经用户批准后核查影响面立票。 -->
<!-- 基线：feat/learn0928 尖端 06091fd45（s1–s7 并回后；412e3ba0b..HEAD 前端
      触点仅 s1 i18n / s6 测试基建路过，本波触点零结构变化）。 -->

## 用户裁决（2026-11-11）

1. **方向 = 一张真话面**（工作区结构条），其余界面从它取真或闭嘴；后端只出
   机器面（attempt 数值），人面文案全走前端词典（c3 契约延续）。
2. **v4 对话框卡片披露为可裁项**：裁撤不影响 v1–v3（票面自注）。
3. **terrain 门前删键不并入本波**：语义安全未裁决（模型多吐 `type` 时地形
   其余部分是否可信未知），留独立票待 owner 拍板——真机收益参考：本课程若
   terrain 删键，12 分钟回票循环可缩到 ~2 分钟单轮。

## 缺口 → 票映射（证据均为 2026-11-11 真机 + 渲染探针实码）

| 缺口 | 事实 | 归票 |
| --- | --- | --- |
| G1 谎言面：图课程 HTTP 秒回后悬浮 Pill 挂「✓ 生成完成」（绿标），结构实际再跑 12 分钟 | 渲染探针 S3：`✓ 生成完成 / 标题 / 查看`；Pill 绑 `generation.status`（`index.tsx:446-455`），graph 流秒翻 completed | **v3** |
| G2 哑巴面：工作区徽章 running 期间只有静态「生成中…」，耗时/轮次/拒收零披露 | `LearningGraphWorkspace.tsx:560-563` Tag 单值；重生成循环只写 WARN 日志（12:08:01 实录），UI 全程不可见 | **v2** |
| G3 失忆面：刷新后徽章「结构未生成」+ 可点生成按钮（点了才 409 toast） | `structureRunning` 仅 WS 帧驱动（不重放）；d7 `refreshRegistryBusy`（`:315-350`）快照补位覆盖 basis/mount/repair/lesson 四类，**漏 `structure` 本尊** | **v2** |
| G4 无轮次面：attempt 不存在于任何机器面 | 注册表无 attempt 字段；`MAX_ATTEMPTS=3`（`structure_gen.rs:19`）循环 `:89` 只 tracing | **v1** |
| G5 防呆缺：结构生成中「补充节点/生长」按钮可点 | `:611` disabled 只看 `growth_running`；后端零节点停摆臂只拦自动臂，显式 POST force 恒过 | **v2** |

## 影响面核查关键事实（2026-11-11 实码核实 @06091fd45）

- **v1 有现成模板**：s4 的 `resolved_course_id` 三件套 = `TaskRecord` 字段
  （`generation_tasks.rs:102`）+ guard setter（`:203-204`）+ registry setter
  （`:507`）+ 初始化 None（`:386`）。attempt 照抄该形状 + `GenerationTaskView`
  暴露（serde 可选缺省 None，老前端兼容；s4 未暴露 view 因其为后端内部终态
  解析用，attempt 是 UI 机器面必须暴露）。注册表进程内存态，无迁移。
- **attempt 回写路径**：循环在 `generate_structure_artifacts` 内
  （`structure_gen.rs:89` `for attempt in 1..=MAX_ATTEMPTS`），task guard 在
  调用侧 `run_structure_generation_task`——经回调参数传入（`&dyn Fn(u32)` 形
  状），调用面全量仅 2 处：`learning_graph.rs`（主路，闭包接
  `task.set_attempt`）+ `service/tests.rs`（noop）。
- **取消端点已暴露**：`api.ts:223`
  `/api/learning/generation-tasks/{key}/cancel`（d7 加）——v3 取消纯接线。
- **轮询先例**：c2 `useCourseCreation` 收敛轮询（5s、终态即停、假数据源可注
  入）+ d7 `refreshRegistryBusy` 快照三源并集形状——v2/v3 照形状走。
- **成功路径自洽**（核查排除现存 bug）：`structure_completed → load() →
  `:166` view.structure 非空即清 running`；失败/取消路径显式清（`:181-185`，
  注释注明快照 null 无法自行消解）。
- **词典真值（现状）**：`genBgDone`=「生成完成」、
  `learningGraphStructureGenerating`=「生成中…」、
  `taskKindStructure`=「知识结构生成」——G1/G2 文案面即此。
- **真机时间线（01a1c8c3）**：12:00:41 begin → 12:08:01 attempt 1 契约门拒
  （gd.terrain.keys 多吐顶层键 `type`）→ 12:08:01/12:08:14 重生成两路 →
  12:13:12 done（751315ms / 5 调用 / 43017 出字符 / 41 块 7 门 16 标注）。
  用户 12:08:45 进入工作区，全程只见「生成中…」。

## 施工顺序

v1 → v2 → v3（v2/v3 消费 v1 的 attempt；v3 理论上可只做 elapsed 不强依赖，
但统一等 v1 落地后施工避免二次改）。v4 随时可裁。

## 非目标（全波）

- **terrain 门前删键**：独立语义裁决票（见裁决 3），本波不立不做。
- v2 描述流/知识库流的 Pill 与对话框呈现零变化（含既有「生成完成」语义——
  v2 流 HTTP 终态即真终态）。
- companion 及一切学习模块之外的面零改动。
- WS 结构帧不加字段（attempt 走快照轮询通道，帧保持 best-effort 现状）。
- AppError 错误文案 i18n（error-surface contract 管辖）。

## 真机目验欠账合并说明

本波真机验收与历史欠账（c1–c3 三票目验项 + s1 键渲染 + s4 恢复态收敛）同批
执行：建一门新图课程走完全程（含一次切页往返），一次覆盖两波判据。

## 收图记录（2026-11-11）

- 施工：v1 = `4b8ef6466`；v2 = `36d9ec8f0`；v3 = `522ea2323`+`4803a6957`；
  v4 = `3807ce671`。单票一代理（v1 先行，v2∥v3 并行，v4 收尾），worktree
  各自分支，merger 依次并回；tracker 随 `1efea86fa` 入 git。
- 双轴评审（Standards/Spec 并行子代理）：**零硬违规、零越界**——Spec 逐项
  核实：attempt 仅结构循环（growth/lesson None）、G3/G5 落地、落库退场、
  per-key 取消接线、恢复态经 buildRestoredRequest 覆盖、v4 单一轮询源。
- 评审修复 = `e36565525`（F5 attempt 启动初始化）+ `73983597c`（F1 格式器
  单源 / F2 Pill 单块 + GraphPillFacet 两相 / F3 占位符全对称断言 / F4 装配
  源断言五钉）。留痕不修：structureLine prop 透传（轻度 Middle Man）、两处
  同形状轮询不抽公共 hook（均为票面/评审判可接受的 judgement）。
- 验证（集成树 bb20d5261 会话独立复验）：前端 learning **242 绿**、后端
  nomifun-learning lib **963 绿**、typecheck 0 错（s 波后基线）、gen:i18n/
  check:i18n 绿（12372 keys）。
- 真机欠账合并批：v 波四票目验 + c1–c3 + s1/s4 + 口语化波（o1–o6，已立票
  待 v 波并回后开动）。
