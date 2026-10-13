# allo 实施后对应面盘点（二轮吸收对照）

> 调研日期 2026-10-13；仓库 `C:\Users\test\Desktop\work\allo`，分支 `feat/learn0928`，
> 实施系列 269f67eb0..HEAD（i01–i08 + 评审轮，16 提交，**未推送**）。工作区即最新。
> 为票 [allo 实施后对应面盘点](../tickets/r2-allo-post-impl-surfaces.md) 的结案资产。

## 1. 教练批管线（i06/i07 后现状）

一条管道：`kick_growth`（触发：就绪 < 3 自动 / force 手动；停摆仅空图一臂）→
`run_growth`（pending 批行 + WS 终态帧）→ `growth_body`（上下文 → 起草/重裁 → 门 → 评审
→ 落库），全在 `crates/backend/nomifun-learning/src/service/learning_graph.rs`：

- **重试循环** `growth_body` :504-648：`for attempt in 1..=GROWTH_MAX_ATTEMPTS`（=3，:18）；
  首轮 `coach_draft`、其后 `coach_redraft`（带上一稿，修订纪律「点修不是重写」，:497-527）；
  解析/形状失败注入条款报告重裁（:550-565）；结构门打回 → growth_redraft WS + redraft
  （:574-592）；评审打回 → 同上（:593-617）；门与评审都过即 break；空 nodes 批删除 pending
  行、outcome=idle。**无同指纹早停**（对照 learnhub 6cddb21 缺口）。
- **`gate_batch`** :928-981：从 DB 取全局已教账（存活课程 teaches 折叠）、登记表全名集合、
  课程内标题 → `validate_batch`；深度水位门已退役（g3，:978-979 注释）。
- **教练上下文九块**（g3，`learning_graph/mod.rs` `CoachContext::render` :312-414）：
  学习目标+程度/基础声明 → 结构参考材料（全量块摊开，`structure_view::render_reference_lines`
  :321-377）→ 前置地形门口（首批重点+三行首批纪律）→ 当前就绪 → 花名册（`⚑` 卡点）→
  已教概念账（canonical@档位）→ 覆盖读数（纯展示）→ 行为摘要 → 概念登记表切片（限 200 行）。
- **COACH_SYSTEM**（`mod.rs` :253-310，内联常量，g4 裁决「契约迁移与提示词内联解耦」）：
  档位 + assumes 覆盖律（同批兄弟 teaches 与本批铸名不构成供给）+ 起点纪律 + 节点纪律
  （单次会话动作句、常规 5-30 分钟封顶 60、难度 1-5 如实申报、**新授概念稀缺：一节点通常
  1 个核心概念至多 2 个**）+ 概念纪律 + 第一批（空账本 ⇒ assumes 只可空或课程外常识）+
  行为信号 + 数量契约（补到 7）+ 提交前自查 7 条 + 纯 JSON。
  提示词里**没有**「装箱/先推后练/首现引入/渐隐/est=XP」这类词。
- **结构门 `validate_batch`**（:165-242）：`ng.mints.collision`（canonical 或别名撞登记表，
  大小写折叠）、`ng.title.duplicate`、`ng.refs.resolvable`、`ng.assumes.uncovered`（就绪即
  发布）。形状条款在 `growth_contract::validate_proposal`（ng.* 编号，schema 单源互钉测试）。
- **概念评审门**（:529-634）：`CONCEPT_REVIEW_SYSTEM` 四判据——①挂号充分 ②前提足够
  （已教账∪assumes 为已知全部，本批 teaches 不算——**与 learnhub 层2 修正已对齐**）
  ③命名与内涵一致 ④难度诚实与阶段/档位相符（形式化/证明类不得压 1-2 档）；advisory-but-
  blocking once（失败降级 PASS）；reject 取前 8 条注入重裁。
- **结构化输出基建**（i07）：`LearningCompleter::complete_json_with_thinking`（不支持即
  Conflict）→ `generation::complete_json_coach`（thinking ON、480s）；四类调用全在 json
  接缝；schema 单源 `growth_contract.rs`（当前只被测试消费，为 json_schema 模式预置）；
  **容错 deserializer 已退役**（字符串 minutes / 中文档位 = 违约）。

## 2. 铸名门

**已覆盖**：批内唯一（canonical∪别名互撞，`ng.mints.batch_unique`）；与登记表撞名
（canonical 或别名逐条 lowercase 比对，`ng.mints.collision`）；引用可解析；落库复用
（`apply_batch` :1011-1054 事务内重查，命中复用 concept_id，幂等防并发）。

**缺**：
- 无定义归一化/近名判定（learnhub `defnear.rs`/`nearname.rs` 对应物）——定义近义不判复用
  改挂，只有精确名撞。
- **复用路径不落别名**（learnhub v2.5 闭环缺失）：经名称解析复用既有概念时，提出的新名字
  不写入 aliases。
- 登记表名字池不出现在教练输入（无「概念别名池」行），教练无选名裁量条款。
- `deprecated` 口径不一致：门侧读登记表过滤 `deprecated=0`（service :1799、:1806），但
  `apply_batch` 的复用查询与解析查询**未过滤 deprecated**（:1014-1019、:1086-1090）——
  对已退役概念铸名会在落库侧静默挂到退役行。
- DB 层无 canonical 唯一约束（067 注释明示「别名联合唯一由代码层校验」）。

## 3. 结构生成管线（i02）

模块：`structure_gen.rs`（门面）、`structure_contract.rs`（gd.* 契约+双 schema）、
`structure_prompts.rs`（提示词资产装配）、`structure_view.rs`（读侧投影）、资产
`prompts/structure/{goal-decomposition.md, prerequisite-terrain.md, golden/}`。

- 双调用：structure → terrain 串行（thinking ON、temperature 0.2、16384 tokens）；门口
  对账 `reconcile_doors`（悬挂记 `gd.domain.doors`）。
- 重试：`MAX_ATTEMPTS`=3，任一门红 → 两份一起重生成 + 逐条款报告注入（**无同指纹早停**，
  对照 learnhub 缺口）；警告级（blocks 80<n≤100）只透出不重试；provider 不支持 json_object
  直接失败不降级。
- gd.* 条款清单（`docs/contracts/learning-structure-artifacts.md`）：structure 侧
  gd.parse / gd.keys / gd.frontmatter / gd.domain.required / gd.domain.doors /
  gd.spines.count / gd.spines.required / gd.blocks.count / gd.blocks.fields /
  gd.blocks.one_name / gd.blocks.slug / gd.blocks.spine_ref / gd.blocks.quota /
  gd.edges.endpoints / gd.edges.type / gd.edges.tier / gd.edges.dup / gd.gaps /
  gd.quality / gd.no_schedule；地形侧 gd.terrain.keys / gd.terrain.frontmatter /
  gd.terrain.doors。
- 落库：双产物过门后 `snapshot_envelope` 单 UPDATE 原子写 `structure_json`；
  WS structure_started/completed/failed；`regenerate_structure` 从存量恢复输入后台跑。

## 4. 内容生成（传统课程 + 学习图节点）

- 三阶段：蓝图（BLUEPRINT_SYSTEM）→ 节大纲（section_outline_system）→ 逐节正文 → 题目
  （LESSON_SYSTEM：九题型、「只考节正文教过的东西」、estimated_minutes 10-30 封顶 60）。
- **写作面纪律现状**：节大纲「动机→概念→例题→小结」可学序 + 练习收尾；正文 VISUAL-FIRST
  承诺兑现制 + 密度纪律（短段落/禁 filler/图优先）+ 排版纪律 + 正文内不布置练习 + 篇幅
  预算；三种教学风格（Standard/Socratic/Feynman）。
- **写作面闸门**：`SectionPack::validate_body`（非空、按节型最小字数、禁 `###`、纯文字硬顶
  2000、demo 必须有可视化、承诺兑现逐项、practice ≤250）；`lesson_draft::audit_findings`
  （节违约/题量地板/estimated_minutes 5-60 警告）。
- **没有**：「先推后练 / 首现引入 / 渐隐（fading）」字样的纪律（全仓 grep 无命中）。
- **节点教学上下文**（learnhub node_ctx 对应物）：`LearningService::graph_node_topology`
  （`service/lesson.rs` :476-604）+ `render_prerequisite_path`——前置契约（档位即停讲深度）、
  逐条 assumes 概念 canonical@档位 + 登记表 definition、供给节点（标题+purpose+已生成节
  摘要）、禁止清单（防超纲下游）；注入 `lesson_user_text`（`lesson_content_loop.rs`）。
  对照 learnhub node_ctx 缺：**首现判定（祖先闭包）、投影词表、引用名开讲**。

## 5. est / minutes 量纲现状

- 教练批：minutes 字段（1-60 门 / 提示词常规 5-30、极困难 60）→ `estimated_minutes`。
- 难度：1-5 门，缺省 2 落库；水位门已退役，诚实性交 AI 评审第 4 条。
- 打卡（checkin）：**复习事件计数**（缺省 15），学习日 = 本地 02:00 翻日，与分钟零换算。
- XP：**不在学习模块**——companion crate（`gamify.rs` Lv=sqrt(xp/100)；`add_companion_xp`；
  伴随对话每轮 +2）。`nomifun-learning` 内无 xp 字样。
- **结论：不存在单一量纲宣言**；分钟、XP、打卡三套量纲互相独立无桥。

## 6. CONTEXT.md 词条

**没有**「道与术 / 概念不可学完 / 概念投影 / est 与 XP 量纲」任何条目（全仓 grep 无命中）。
现有邻近词条：Lesson（≤30/封顶 60）、Structure（课程边界）、Concept Registry、Concept Web、
Tier、Ready、Growth（九块上下文+≤7 批）、Coverage Gauge（纯展示）、Stall（仅空图臂）、
Soft Gate、Batch。

## 7. 冒烟入口（i08）

`nomifun-app/src/commands/learning_smoke.rs`，`nomicore learning-smoke` 四子命令
（structure / json / grow / loop）；与应用同组合根接线；缺省从零临时 SQLite、`--db` 可指
库；`ArchivingCompleter` 观测存档 `observation/learning-smoke/<run>/`。机械判据：structure
（重验双契约+对账，失败 exit 2）、json（读数面，唯一硬判据全灭 0/n）、grow/loop（首批
assumes 全空、就绪即发布、铸名全局唯一；确定性学习者替身驱动）。

## 8. 重点结论（二轮吸收对照）

1. **教练批**：已迁 json 接缝 + ng.* 契约 + 3 轮带反馈；**缺早停与容量守卫**；评审「同批
   兄弟不算供给」已对齐 learnhub 修正；无装箱类交付账机制（一轮已裁不引入）。
2. **铸名门**：精确名撞（含别名）已覆盖——**learnhub #79 的名字面兜底 allo 已有同效**；
   增量 = 别名入池闭环、deprecated 口径、近邻判定（未定）。
3. **写作纪律**：有视觉承诺/密度/闸门；**缺序列面纪律**（先推后练/首现/渐隐/投影名）。
4. **量纲**：三套独立无桥；「est = XP = 1 分钟专注」为零。
5. **教学上下文**：graph_node_topology 较完整（definition+档位停讲+供给节点摘要+防超纲），
   对照 node_ctx 缺首现/词表/引用名三投影。
