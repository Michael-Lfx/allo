# Map: 第八轮吸收波——learnhub #205 节点意图与教学范围（z01–z03）

<!-- wayfinder:map -->
<!-- 状态：**已到达目的地（2026-11-05 关波）**。z01–z03 全关（Resolution 在票尾），
      集成分支 feat/learning-absorb-8 尖端 495b3ce60（z01 三提交 1ce098fd1 并回 +
      z02 69c3f2d98 + z03 c58b23262 + 双轴评审修复轮 495b3ce60，全推 origin）；
      合成树 learning 855 / ai-agent 1168 / nomifun-app 465 全绿；真机两臂串行
      EXIT=0：grow `[brief: rows=5 intent_avg=40 scope_avg=42]` +
      `[fragments: total=127 rows=127 unhandled=0 degraded=false]`，content
      判据全过（scope_findings first_pass_hits=0 + legacy 负样本 0/0）。
      并回 feat/learn0928 等用户通知（推送铁律）。真机凭据链：absorb8-smoke/
      工作目录（data/encryption_key ← Nomi-dev，provider 行 ← absorb-7
      quality.db，bun:sqlite 搬运）。 -->
<!-- 基线：feat/learn0928 尖端 939443a1d（本波立票提交，已推 origin）。 -->
<!-- 参照点：learnhub main 20f1dcf（2026-11-04 18:12 +0800）。 -->

## 来源

learnhub `22d016d..20f1dcf` 共 12 个提交，按段归组：

| 段 | 提交 | 机制 |
| --- | --- | --- |
| 引擎核心 | `00d7813` | nodes 必填 intent/scope（`ng.node.brief` ≤200 字）+ 机械点名四源扩面带来源标记 + 评审「范围外」三件机械核 + 落位两字段旧档容忍 + 里程碑日志 + 契约 v1.20 四提示词金样 |
| 内容装配 | `fda5469` | 骨架行声明面两行（缺席省略）+ scope_findings 报告-only 词面核对 + 声明优先条款 |
| GUI | `43ed292` | 详情页「为什么学/学到哪」两行（缺席整段不渲染）+ NodeView 投影 |
| code-review 收口 | `0b45618`、`525363c`、`b55dd31` | 透传措辞/否定词表单源/ui-vocab 归位 + 格式化 + CI 扫雷 |
| 软条款 | `43056f6` | 拆步自检——拆分产物避免再造新并列（对照型豁免保留，零机械门） |
| 真机病历修正 | `20f1dcf` | **范围外机械核语义：按词跨源多行合法**（单行硬比误杀 → 两连重试 → Degraded 保守放行整场评审被跳过的病历收口） |
| 文档 | `b5d433f`、`3b07214`、`72ae7b1` | 词条与模块地图 + smoke 真机基线两条 |

## 总约束

1. **范围外机械核直接带 `20f1dcf` 最终语义**：合法性 = 该词有 intent/scope
   来源的机械在场行（按词判断、跨源多行合法），不踩「行报与机械首行一致」
   单行硬比；单测钉住病历形态（同词双源裁决不被核伪）。
2. **purpose 保留不动，另加 intent/scope**（推荐项默认裁决，可复议）：
   allo `purpose` = 学完能做什么（outcome/定位面，已有五个消费点：
   lesson_user_text 节点定位行、前置/禁止清单摘要、检索词条、LessonStudy）；
   learnhub `intent` = 为何有此点（创建意图）；`scope` = 教什么/不碰什么
   （边界声明）。三者语义分立，不合并不改名。
3. **迁移只编辑 070 基线**：`070_learning_domain_rebuild.sql` 仍未发布
   （PR#350 对 main CONFLICTING 未合并），learning_lessons 两列在该文件
   「原 073_learning_lesson_difficulty」节同区追加（`:271-274` 先例同形），
   不新增迁移文件；迁移测试同步。
4. **范围外裁决跟随既有 `review_fragments_enabled` 开关**：off 臂无点名
   清单则无三值裁决，零新开关；scope_findings 恒开 report-only（j07 审计
   观察先例，零拦截零 LLM 零迁移零开关），仅图路径限域。
5. **优先复用现有设施**：findings 通道（j07 warning 形状）、渲染纯函数
   先例（render_known_names_line 缺席省略）、旧档容忍先例（est/batch →
   Option/空串）、点修守卫（ProposedNode derived PartialEq 自动含新字段）。
   不新造第二套。
6. **真机臂严格串行**；涉写读闭环的票跑 `nomicore learning-smoke` 对应档。
7. **开工先复核 file:line**（y 波惯例）：票内 allo 行号来自 2026-11-05
   盘点（基线 939443a1d），learnhub 参考行号固定按 `20f1dcf` 读；
   **新会话第一步必须先读 learnhub 参考实码再动 allo**。

## 影响面盘点（2026-11-05 实码核对）

| 面 | allo 落点 | 现状 | 落差 |
| --- | --- | --- | --- |
| 契约门 | `growth_contract.rs:20` NODE_KEYS | 六键无 intent/scope | +两键 +`ng.node.brief` 条款 |
| 生成结构 | `learning_graph/mod.rs:168` ProposedNode | title/purpose/est/difficulty/teaches/assumes | +两 Option 字段（PartialEq 自动进点修守卫 `growth_retry.rs:177`） |
| 生长提示词 | `mod.rs:488`（JSON 形状示例）+ `:513-550`（节点纪律） | 无两字段 | +要求与写法条款 +归一化 trim（`:723` purpose 同款） |
| 机械点名 | `fragment_verdict.rs:175` derive_naming_list | **只扫 mints/aliases 定义源**、`Vec<String>` 无来源标记 | 扩 title∪intent∪scope 三新源 + 片段带 FragmentSource（learnhub #205 前的 title 源 allo 也一直缺，顺带补齐） |
| 裁决词表 | `fragment_verdict.rs:47` FragmentRuling | 两值（噪声/名词） | 三值 +范围外 + `check_out_of_scope_row` 三件机械核 |
| 评审提示词 | `mod.rs:905-917` 碎片裁决条款 + `:858` CRITERION_ONE_V2 框定 | 词表两值、框定=标题与 purpose | +范围外裁决规则 +框定补 intent/scope |
| 落库 | 迁移 070（`learning_lessons` 列在 015/040/070） | 有 purpose/difficulty 列 | 070 节内 +intent/scope 两列（NOT NULL DEFAULT ''，空=旧档） |
| apply | `service/learning_graph.rs:3486` INSERT | 六字段 | +两列两 bind |
| DTO | `models.rs:2653` LessonView / `:345` LessonPack | 无两字段 | +两字段（LessonPack serde default 容旧导入包） |
| DTO 读出 | `service/course.rs:346/:492/:529/:384` | SELECT 无两列 | SELECT +两列 +构造补字段 |
| 内容装配 | `service/lesson.rs:1120` SELECT + `:1177` GraphLessonContext 构造 | 无两字段 | SELECT +两列、context +两 Option |
| 骨架行 | `nomifun-ai-agent/lesson_content_loop.rs:752-845` lesson_user_text | 节点定位行=purpose | 节点定位行后插两行（缺席省略） |
| 声明优先 | `lesson_content_loop.rs:804` teaches_tiers 行（D4 深度角） | 无声明面 | 两字段在场为边界/意图第一真相 |
| findings | `lesson_draft.rs:1290-1380` 审计观察面（j07 概念面/R9 同缝） | 无范围核对 | scope_findings 新模块接线（报告-only） |
| UI | `ui/src/renderer/pages/learning/components/LessonStudy.tsx:791`（purpose 段） | 无两行 | 两行展示，缺席不渲染 |
| TS 镜像 | `ui/src/renderer/pages/learning/types.ts:311` Lesson | 无两字段 | +两 string 字段（空串=旧档） |
| smoke | `nomifun-app/src/commands/learning_smoke.rs`（4240 行） | grow/content 臂无相关判据 | grow：必填首证+落库读回；content：两行渲染+旧档省略+scope_findings 播种 |

## 优先级与票表

三票，依赖序 z01 → （z02 ∥ z03）。

| id | 票 | 档 | 阻塞/顺序 |
| --- | --- | --- | --- |
| z01 | 节点声明面两字段——契约门、生长提示词、落库、DTO、UI | **高**（底座） | 先做；z02/z03 都依赖它的字段 |
| z02 | 点名四源与范围外裁决——来源标记、三值裁决、三件机械核、评审条款、拆步条款 | **高** | 排 z01 后（依赖 ProposedNode 两字段）；与 z03 无文件交集可并行 |
| z03 | 内容消费面——骨架行声明两行、声明优先条款、scope_findings、旧档负样本 | **高** | 排 z01 后（依赖 DB 列与 context）；与 z02 无文件交集可并行 |

## 文件冲突面（并行编排依据）

- `learning_graph/mod.rs`：z01 碰 ProposedNode 结构体+生长提示词+归一化
  （`:168-186`/`:480-550`/`:700-730`），z02 碰评审条款+判据框定
  （`:856-950`）——同文件不同区段，并行可合并，merger 串行处理。
- `service/learning_graph.rs`：z01 碰 apply_batch INSERT（`:3486`），z02
  碰 derive_naming_list（`:3316`）——不同函数，无冲突。
- `service/lesson.rs` **只归 z03**（generate_graph_lesson_content SELECT
  与 context 装配）；`service/course.rs` **只归 z01**（LessonView 读出）。
- smoke：z01 碰 grow 臂，z03 碰 content 臂——不同档。

## 实施编排

- 阶段 A：z01 单独先行（底座字段贯通契约→落库→DTO→UI）。
- 阶段 B：z02 ∥ z03 并行（单票一代理，五轮惯例），merger 串行并回。
- 每票一提交（或按内聚拆少数几提交），并回 `feat/learn0928`；
  真机验收按票串行，不并发挤兑 provider。

## 验证总纲

1. 学习 crate：`cargo test -p nomifun-learning`；z03 触 ai-agent 加
   `-p nomifun-ai-agent`；z01 触迁移基线加 `-p nomifun-db`（单线程——
   并行有 `init_database_memory` provider UNIQUE flake 存量）。
2. 前端（z01）：`bun run typecheck`；新增用户可见文案先 `bun run gen:i18n`
   再 `bun run check:i18n`；`bun run check:theme`/`check:dead-css`。
3. 聚合门：`cargo check --workspace && bun run check`（PR 前）。
4. 真机（串行）：z01 grow（intent/scope 必填首证+落库读回两列非空）、
   z02 grow（范围外裁决读数行）、z03 content（两行渲染+旧档省略+
   scope_findings 播种负样本）；各档 `EXIT=0` 且读数行在场。
5. 存量基线：typecheck 55 错（学习 0）、db 并行 flake——别误判。

## 雾区（本波后置项与触发条件）

- **里程碑日志**（`00d7813` 生长批开始/落盘里程碑日志）：allo 观测面已有
  ArchivingCompleter + calls.jsonl + phase 事件，边际收益低，不吸。
- **ui-vocabulary 用户面词**（`43ed292`）：learnhub 专属词表体系；allo
  i18n 双语键自足，不吸。若实施中出现「学习节点/学习点」类用词分歧再议。
- **learnhub 四提示词金样/回放夹具体系**：allo 提示词资产三闸另有先例
  （j08），本波条款走既有金样/围栏流程，不搬 learnhub 夹具。
- **graph_node_topology 前置/后续/禁止清单摘要带 scope**：消费面收敛
  （learnhub scope 消费=骨架行+scope_findings），不做；真机若暴露下游
  写作越界复发再立票。
