---
id: j01
title: "文本与命名底座：est 统一 + 教义波 + 契约文本四项"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# j01: 文本与命名底座

**What to build:** 全部纯文本/命名层变更一次落位，为后续票清屏。

**实现决议:** 二轮 g1（命名面）、二轮 g4（全部）、三轮 t4（全部）。

**Blocked by:** None。

**Status:** ready-for-agent

- [ ] **est 命名统一**：`GenerateCourseRequest`/DTO 字段、`ng.*` 契约键
      （`ng.nodes.minutes` → `ng.nodes.est`）、`COACH_SYSTEM`/评审提示词措辞、
      `ProposedNode.minutes` → `est`、前端 `types.ts` 镜像与 learningApi、learning-smoke
      全部对齐 est（语义 = 专注工作量尺寸锚）；**DB 列 `estimated_minutes` 原地不动**
      （列注释补语义）；测试夹具同步
- [ ] **CONTEXT.md 三词条**：道与术 / 概念不可学完 / 概念投影（域中立，含 learnhub
      实证注记；「概念投影」与二轮 g3 别名闭环互引）
- [ ] **COACH_SYSTEM 条款波**（域中立改写）：道位条款（一条线只有操作台阶时补「为什么
      成立」位）/ 重访合法（同概念多节点反复教合法，同一角度同一深度的重复没有价值）/
      投影措辞（teaches 写本次到达的理解深度，assumes 按实际依赖深度）/ 弧线跨批投影 /
      对照限同轴 / 动词多样谱 / 首批特则（首批单一概念或单一最小操作；常规批维持
      1–2 核心概念）/ 同批不重教（前置与后继不同批）/ 悬空技巧 + 真实需要三问 / 定义
      文本前提闭包 / 批容量红线（修订不得超 7，宁少拆不超限——配 j03 容量守卫）
- [ ] **CONCEPT_REVIEW_SYSTEM**：判据②补禁则句（批内兄弟教出的概念不算已教，不得建议
      挂 assumes）
- [ ] **方法论节**：`docs/architecture/learning.zh.md` 新增「提示词纪律方法论」——放宽
      必配收窄判据 / 修偏差优先删预设少加规定 / 判据只放在能看到信息的层（各配 learnhub
      病历出处）
- [ ] **契约文本四项**（三轮 t4）：R9 叙述声部 + 跨课预告废除（条款文本与评审判据观察
      面，提示词位随 j07 落）/ 门位口径 v2.10（`goal-decomposition.md` +
      `prerequisite-terrain.md`）/ 术语纠偏 v2.8（契约文档块定义改「主题单元」+ 全仓确认
      无转正/收敛率残留）
- [ ] `docs/contracts/learning-structure-artifacts.md` version bump（四项随批注记）
- [ ] 验证：`cargo test -p nomifun-learning`（ng.* 键改名后全绿）+ `bun run typecheck`
      （learning 路径零新增错误）+ `bun run check:i18n`

**Resolution:**

**速览**：全项落地（feat/learn0928，本票单提交）；实现二轮 g1 第 6 项（est 命名）、
二轮 g4 全部（教义三词条 + 提示词波 + 方法论）、三轮 t4 全部（契约文本四项随批）。

- **est 命名统一**（语义 = 专注工作量尺寸锚，1 est 分钟 = 1 分钟专注）：
  - 图管线：`ProposedNode.minutes` → `est`（LLM 键与 Rust 字段）；`ng.nodes.minutes`
    → **`ng.nodes.est`**（条款键 + 消息 + schema + 金样批夹具）；`MINUTES_RANGE` →
    `EST_RANGE`；COACH_SYSTEM 示例键、修订纪律（「est 原样保留」）、评审「不算问题」
    措辞同步；落库绑定与花名册注释跟随。
  - HTTP/WS 面：`LessonView`/`GraphNodeHistoryView` 字段与序列化键 → `est`；
    `learning.lesson-generation` 事件载荷键 → `est`（nomifun-learning 三处发射面）；
    前端 `types.ts` 镜像、ipcBridge 事件类型、LearningGraphWorkspace/LessonStudy
    消费点、导入样例 `EMPTY_PACK` 全部对齐。
  - 传统管线（本 crate 内）：`LessonPack.estimated_minutes` → `est` **带
    `alias = "estimated_minutes"`**（老导入包双读不回归，新增
    `est_key_tests::lesson_pack_reads_new_key_and_legacy_alias` 钉住）；导入校验与
    落库绑定跟随；`ActivitiesOutput` → `est` + legacy fallback 的 `LESSON_SYSTEM`
    提示词键与规则行改 est；de helper 更名 `de_est_or_default`。
  - **DB 列 `learning_lessons.estimated_minutes` 原地不动**（列名无用户面）——SQLite
    无列注释机制，「列注释补语义」落为：契约文档 §8 注记 + 改名 DTO 的文档注释
    （`ProposedNode::est`/`LessonView::est`/`GraphNodeHistoryView::est`/`LessonPack::est`
    均注明列名保留与 est 语义）。
  - learning-smoke 零直接引用（四档全走库函数，随库自动对齐）；`cargo check
    --workspace` 干净确认无跨 crate 夹具断裂。
- **CONTEXT.md 三词条**（插 Tier 与 Ready 之间）：Dao And Shu（道与术，标尺对 +
  learnhub 30 节点实证注记）/ Concept Unfinishable（概念不可学完，「门全开而行为
  缺失」实证注记）/ Projection（概念投影，teaches/assumes 写本次深度 + 别名闭环
  互引 j04）。
- **COACH_SYSTEM 条款波**（域中立，全织入现有分块）：投影措辞（【档位】新增「本次
  投影」条）/ 道位配平 + 悬空技巧与真实需要三问（【节点纪律】两条新条款）/
  弧线跨批 + 同批不重教（【节点纪律】合一条）/ 对照限同轴（【节点纪律】）/
  动词多样谱（删预设不加规定写法）/ 重访合法强化（螺旋重现条并入「同角度同深度
  重复才无价值」）/ 定义文本前提闭包（零基础校验扩到节点文本引用与预设）/
  首批特则（【第一批】单一概念或单一最小操作）/ 批容量红线（【数量契约】修订
  不得超 7、宁拆不超限——j03 容量守卫的提示词侧）。
- **CONCEPT_REVIEW_SYSTEM 判据②禁则句**：批内兄弟教出的概念不算已教，不得建议挂
  assumes（自造机械矛盾）；真缺前提的正路 = 建议把依赖方移出本批。
- **方法论节**：`docs/architecture/learning.zh.md` 新节「提示词纪律方法论」三条
  （放宽必配收窄判据 c05a43b→4fc663b / 修偏差优先删预设 1bac1c7 / 判据只放在能
  看到信息的层 3edbe5a+6cddb21），出处引二轮 r1 报告。
- **契约文本四项**（`learning-structure-artifacts.md` bump v1.0 → **v1.1**，新增
  §8 随批注记）：门位口径 v2.10 采纳——§3 地形语义段改写（真实起步地板 + 地板
  以上全进结构 + 地形收缩为地板以下与临门自检）+ 两份结构提示词正文条款（金样
  `LEARNING_UPDATE_GOLDEN=1` 重生成，diff 恰为两条新增）；术语纠偏 v2.8 确认
  归档——块定义已是「原子教学主题单元」口径，全仓 grep「转正/收敛率」零残留
  （命中均为无关语义：发版转正/动画转正/批次档案转正）；R9 叙述声部 + 跨课预告
  废除为内容面条款，**随 j07 落内容契约位**（§8 注记，不在结构契约提前落位）。
- **实施偏差与边界记录**：①`LessonOutput.estimated_minutes` **保留旧名**——它是
  跨 crate trait 边界类型（nomifun-ai-agent 实现生成引擎），serde 键不出 LLM/
  HTTP/前端任何用户面，改名会拖着 ai-agent 四处机械改动且超出本票验证梯；内部
  命名对齐留内容管线波（j07 本就要动 lesson_content_loop 提示词面，`set_estimated_minutes`
  工具 op 与 agent 侧提示词的 est 对齐随之）。②「 GenerateCourseRequest/DTO 字段」
  票面所列结构体实际无 minutes 字段，按「携带尺寸锚的 DTO 全改」执行
  （LessonPack/LessonView/GraphNodeHistoryView/ActivitiesOutput）。
- **验证**：`cargo test -p nomifun-learning` **238 全绿**（基线 236 + 新增别名双读
  与退役键两测）；`bun run typecheck` 60 错 = origin/main 存量基线
  （videoCanvas/videoGeneration），learning 路径零新增；`bun run check:i18n` 过
  （11376 键 up to date）；`cargo check --workspace` 干净（仅存量 warning，逐一
  核对非本次引入）；金样快照重生成并逐行复核。
