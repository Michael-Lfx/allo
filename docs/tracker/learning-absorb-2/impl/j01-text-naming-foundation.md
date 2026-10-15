---
id: j01
title: "文本与命名底座：est 统一 + 教义波 + 契约文本四项"
type: task
status: open
assignee:
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

**Resolution:**（收口时写）
