# nomifun-learning 学习模块现状调研报告

> 调研日期 2026-10-12；仓库 `C:\Users\test\Desktop\work\allo`，分支 `feat/learn0928`，
> HEAD = `bab7a4a8e`（growth governance），另有 **15 个文件未提交改动**（+912/−205，集中在
> `learning_graph/mod.rs` +515 行、`service/learning_graph.rs` +177 行）。本报告按工作区
> （未提交改动已包含）状态描述。为票
> [allo 学习模块现状盘点](../tickets/r2-allo-learning-status-research.md) 的结案资产。

## 一、建课流程：3 种建课方式；学习图方式是"两步向导"，第 2 步就是多终点选择

### 1.1 前端交互（CreateCourseDialog.tsx）

入口是三 Tab 对话框（`ui/src/renderer/pages/learning/components/CreateCourseDialog.tsx:142-273`，
默认 Tab 描述生成）：

| Tab | 键 | 交互 |
|---|---|---|
| `base` 从知识库生成 | 选知识库 + 可选领域输入 | 一步提交 |
| `description` 描述生成 | 课程简报 TextArea | 一步提交 |
| `graph` 学习图（orangered Beta 标签） | **两步向导** | 见下 |

**学习图向导两步**（`graphStep: 'goal' | 'endpoints'`，`CreateCourseDialog.tsx:58-59`）：

- **第 1 步 goal**：输入学习目标（`learningGraphGoalInputLabel/Placeholder`），主按钮
  "下一步：AI 提议终点"（`:96-100`）。
- **第 2 步 endpoints（= 多终点/多入口选择步）**：AI 提议的终点锚列表，**每条含标题（≤40 字）
  + 程度声明（goal_note）两个输入框**，可逐条编辑、删除（至少保留 1 条，`:238`）、点
  "+ 添加终点"无限追加（`:260-269`）。主按钮变为"创建课程并开始生长"。取消按钮在此步
  语义是"返回修改目标"（`:108-117`）。
- 提交后对话框就地切换为生成/进度视图，**首生长在后台跑**（悬浮指示条可回看）。

向导状态机在 `ui/src/renderer/pages/learning/hooks/useCourseCreation.ts`：
- `:40-44` `creationTab` 三态 + `graphStep` + `draftEndpoints: EndpointInput[]`
- `:114-136` `proposeGraphEndpoints()`：调 `learningApi.proposeGraphEndpoints`，AI 提议失败
  **降级为空列表、用户手填，绝不阻塞建课**
- `:140-164` `confirmGraphCreation()`：提交 `{course_kind:'learning_graph', description,
  endpoints, teaching_style, provider_id/model}`

`EndpointInput` 类型：`ui/src/renderer/pages/learning/types.ts:363-366`（`{title, goal_note?}`）
与后端 `crates/backend/nomifun-learning/src/models.rs:1270-1275` 对齐。

### 1.2 后端管道（两个 HTTP 端点 + 一条后台首生长）

**端点 A：终点提议** `POST /api/learning/graph/propose-endpoints`（`routes.rs:33-34`）→
`LearningService::propose_graph_endpoints`（`service/learning_graph.rs:26-62`）：
1. `analyze_scope`（1 次轻量 LLM 调用，思考关）
2. `propose_endpoints`（1 次思考开 LLM 调用）

**端点 B：建课** `POST /api/learning/courses/generate`（按 `course_kind` 分流，
`service/generate.rs:26-27`）→ `generate_learning_graph_course`（`service/learning_graph.rs:71-199`）：
1. 校验：只允许描述流，禁带知识库（`models.rs:101-118`）
2. 重新 `analyze_scope`（尽力而为）
3. **用户确认过的终点优先**；只有为空时才现场再提议一次（`:103-120`）
4. 单事务落库：`learning_courses`（kind='learning_graph'）+ 一个隐含模块（标题恒"学习图"，
   `GRAPH_MODULE_TITLE`，`:6`）+ 每条终点插一行 `learning_course_endpoints` + 一条**零正文
   标记课时**（`insert_endpoint`，`:2196-2250`——终点不是学习节点，`lesson_id UNIQUE`）
5. `tokio::spawn` 后台并行：`regen_compass`（罗盘初画）∥ `kick_growth(force=true)`（首生长），
   接口立即返回；进度走 WS 事件（`growth_started/completed/failed/cancelled/compass_updated`）

**建课之后的 LLM 调用轮次**（首生长 `growth_body`，`service/learning_graph.rs:423-598`）：
1. 上下文组装（罗盘缺失时现场补画 1 次）
2. 首批专享：`survey_entry_domains` 入口域盘点（1 次思考开，`:438-454`）
3. `coach_draft` 教练起草（1 次思考开，COACH_MAX_TOKENS=32768）
4. 确定性结构门（0 次 LLM）→ 不过则 `coach_redraft`（带门报告+上一稿整批回灌），
   **修复循环上限 4 稿**（`GROWTH_MAX_DRAFTS`，`:18`）
5. AI 概念评审（1 次思考开）→ 不过同样回灌重裁，同上限
6. 事务落库：铸名 → 节点 → 概念网 → 终点裁决 → 批次档案（`apply_batch`，`:964-1137`）

## 二、学习图数据模型（ADR-0009 概念网生长模型）

模块级文档：`crates/backend/nomifun-learning/src/learning_graph/mod.rs:1-18`。核心设计：
**课程不预建 DAG，从若干"终点锚"起步，按小批次生长；依赖关系完全放在概念网里
（teaches/assumes × 档位），没有先修边、没有锁定态——发布即就绪；跨课程就绪由全局概念
登记表自然产生。**

### 2.1 关键常量与类型（`learning_graph/mod.rs`）

```rust
pub(crate) const READY_TARGET: usize = 7;    // :29 就绪补货目标
pub(crate) const READY_TRIGGER: usize = 3;   // :32 自动触发水位线
pub(crate) const MAX_BATCH_NODES: usize = 7; // :35 单批新增上限

pub enum ConceptTier { Know, Apply, Teach }  // :41-66 知道<会用<能教，DB 存 locale 无关代码
pub struct ConceptRefInput { name, tier }                        // :73
pub struct ProposedNode { title, purpose, minutes(1-60), difficulty(1-5), teaches, assumes } // :80-96
pub struct ConceptMint { canonical, aliases, definition }        // :101-107
pub struct EndpointVerdict { title, completed }                  // :112-115
pub struct ProposedBatch { nodes, mints, completed_endpoints, note } // :119-128
```

### 2.2 结构门（确定性，零 LLM）

`validate_batch`（`:160-312`）：批量上限 / 标题合法唯一 / 铸名不撞登记表与批内 / 概念引用
可解析（登记表∪批内铸名）/ **就绪即发布：每条 assumes 必须被全局已教账（既往批次落库的
teaches）按档位覆盖，同批兄弟的 teaches 与本批铸名不算供给** / 终点保护。
`validate_batch_depth`（`:1156-1192`）：**深度水位门**——本批最高难度 ≤ 已落库批次数+2，
且标题含"证明/严格/形式化/公理/ε-δ"者按 ≥4 计（堵虚标）。

### 2.3 就绪集与跨课程逻辑

- `ready_set`（`:1257-1268`）：未满足且所有 assumes 被跨课程已教账覆盖的节点；顺序即推荐序。
- `taught_ledger`（`:1240-1251` + `service/learning_graph.rs:1167-1185`）：concept_id → 全部
  **存活课程**（`deleted_at IS NULL`）teaches 的最高档。
- **R 软闸**（`ready_partition`，`service/learning_graph.rs:1588-1782`）：结构就绪但供给节点
  预测回忆率 < 0.85 的候选，从推荐剔除、出复习建议条（≤3 条）。

### 2.4 生长触发与停摆臂

- **自动触发**：节点完成/跳过后 `maybe_kick_growth`（`:247-269`），就绪 < 3 才动。
- **停摆两臂** `growth_stalled`（`:275-299`）：空图（首生长 force 走不占自动触发）；或课程
  有终点且全部被标记完成。
- **手动触发**：`POST .../graph/grow`（`routes.rs:45-46`），`force=true` 永远豁免停摆臂。
- **行为摘要**（ADR-0009 Amendment 1）：`fold_behavior_digest`（`:1400-1521`）——7 学习日/
  10 节点双条款窗口，正确率趋势、卡点节点（⚑ 标记，"一次至多一个针对性回应节点"）、
  真实保留率（review_log 的 auto/self 评分）。
- **覆盖读数** `render_coverage_gauge`（`:1529-1560`）：已学达标/已跳过 + 概念档位足迹，
  终点完成裁决的依据，"永不自动置位完成"。

### 2.5 concept 与 course/lesson 的关系

- 概念是**全局资产**（`learning_concept_registry`，跨课程唯一，canonical+别名）；
- 节点（课时）通过 `learning_lesson_concepts`（role: teaches/assumes × tier）挂到概念上；
- 终点锚每条同时是一行零正文标记课时，但被排除在就绪候选、覆盖读数、花名册之外；
- 课程删除改为软删除（069），课时/模块/概念网边物理删除，孤儿概念全删时从登记表收紧
  （`service/course.rs:485-520`）。

## 三、可视化现状（重要结论：没有图形化图渲染）

`LearningGraphWorkspace.tsx`（815 行）**不是 canvas/SVG 图谱**，前端也**没有布局算法**。
确认依据：learning 目录下 grep 不到 canvas/d3/reactflow/cytoscape/vis-network；
`ui/package.json` 唯一图形依赖是 `jsxgraph`（`:82`，仅课时内数学图形）。后端注释明说：
"**DAG 画布已不存在**——节点本体就是模块课时，图视图只承载生长语义"
（`service/learning_graph.rs:1518`）。

现在的"可视化"是 4 类 Arco 组件信息架构（`LearningGraphWorkspace.tsx:44-51` 注释即设计说明）：

1. **顶条**：终点锚 chips（可点编辑、closable 删除，✓ 前缀表已完成，`:254-278`）+ 生长中
   徽章 + "补充节点（就绪 x/7）"按钮（`:288-306`）+ R 软闸建议 Alert（`:309-320`）+
   **罗盘卡**（默认收起一行，点击展开 `<pre>` 纯文本 markdown，可手动重画，`:527-599`）。
2. **学习 Tab**：左侧 280px 就绪集导航（主推荐卡 + 其余可学节点按钮列表，`:333-394`）+
   中央选中节点的完整课时学习区（复用 `LessonBlock`，`:396-450`）。推荐位自动跟随。
3. **学习记录 Tab**：批次时间线（倒序卡片，每批 seq/note/status/节点行，`:601-705`）——
   "生长史即课程史"。
4. **概念表 Tab**：Arco Table，列 = 概念（canonical+别名）/ 教·假定引用（绿/橙 Tag + 档位 +
   节点标题 + 状态）/ 其他课程也在教（`:707-813`）。

节点类型只有一种（学习节点 = 课时），外加非节点的终点锚；WS 事件驱动刷新（`:90-103`）。

## 四、数据库迁移（learning 相关全表清单）

| 迁移 | 内容 |
|---|---|
| `015_learning_engine.sql` | 最初引擎（courses/modules/lessons/activities/enrollments/progress/attempts） |
| `029/036/037/038/039/042/043/044` | 自建题/标签/课程任务/填空/题级复习/打卡/归档/待编辑 |
| `040_learning_on_demand_courses.sql` | 按需生成（blueprint_json/samples_json 落课程行） |
| `048_learning_graph.sql` | 学习图 v1：`course_kind` + learning_goal/learning_scope/graph_meta_json + 先修边表 + skipped |
| `050/051/052/053` | 分节正文/节视觉/复习日志/降级 |
| **`066_retire_graph_dag_and_concepts.sql`** | **破坏性**：删先修边表 + 全部旧图课程数据；DROP 旧 per-course 概念五张表 |
| **`067_concept_web_growth.sql`** | 概念网落点：`learning_concept_registry`、`learning_course_endpoints`、`learning_lesson_concepts`（UNIQUE(lesson_id,concept_id,role)）、`learning_growth_batches`、`compass_md` |
| `068_growth_batch_status.sql` | 批行状态机：`status('pending'/'applied'/'failed')` |
| **`069_learning_course_soft_delete.sql`**（未提交） | `learning_courses.deleted_at` 软删墓碑 |
| **`070_learning_lesson_difficulty.sql`**（未提交） | `learning_lessons.difficulty INTEGER NOT NULL DEFAULT 2`（深度水位执法面） |

## 五、AI agent 桥（nomifun-ai-agent/src/learning_completer.rs）

`LiveLearningCompleter`（`:39-44`）实现 learning crate 的 `LearningCompleter` trait
（`completer.rs:17-45`，`complete` + `complete_with_thinking`，均带显式 max_tokens 预算和
model_override）。由 app 层经 `LearningService::set_generation_dependencies` 注入。

- `complete`（`:48-82`）：`one_shot_completion_no_thinking_text_or_reasoning`——思考**关**，
  content 空时回落 reasoning 通道。
- `complete_with_thinking`（`:94-150`）：教练/评审/终点提议/罗盘专用——思考**开**，
  **BadGateway 瞬态错误重试恰 1 次**（间隔 1.5s）。reasoning tokens 与 JSON 回复共享
  max_tokens 上限（所以 COACH=32768）。

结构化输出解析统一走 `generation/parser.rs` 的 `parse_json_object`（剥代码围栏后 serde），
配合大量容错 deserializer（`learning_graph/mod.rs:793-965`）：minutes 可为字符串、概念数组
可为单对象、tier 接受中英文（"知道"→know）、终点裁决可为裸标题字符串。

**各阶段 token 预算**（`generation/completer.rs:27-63`）：blueprint 4096 / scope 8192 /
**coach 32768（480s 超时）** / concept review 16384 / endpoint proposal 16384 / compass 16384 /
普通调用 180s。

## 六、提示词原文（学习图链路，全在 `learning_graph/mod.rs`）

**终点提议 `ENDPOINT_PROPOSAL_SYSTEM`**（`:503-514`）——多终点选择步的提示词核心：

```
你负责为一个新的学习目标提议若干条「终点锚」。终点是学习的方向标记：学习者抵达终点时应当具备什么能力。
只回复一个 JSON 对象，形状如下：
{ "endpoints": [ { "title": "终点名（名词短语，如：独立完成一元二次方程的求解）", "goal_note": "一句程度声明：抵达时能做到什么、到什么程度" } ] }
规则：
- 每条终点必须指向一个可检验的能力状态，而不是「学完某本书」这类过程描述。
- 目标单一清晰时提议 1 条即可；目标含多个相互独立的努力方向时才拆成多条，数量不封顶——但每条都必须是真实独立的努力方向，不为凑数硬拆。
- 终点标题用学习目标的语言书写，20 字以内。
- 只输出 JSON，不要 Markdown 代码块，不要任何解释。
```

**范围分析 `SCOPE_SYSTEM`**（`:427-441`）：输出 `{goal, baseline, scope, blocks[]}`；baseline
缺省视为"彻底零基础"。

**入口域盘点 `ENTRY_SURVEY_SYSTEM`**（`:332-344`）：首批前独立调用，输出 4-8 个底层域，
每域一个 `bottom_rung` 动作句 + `serves`。

**教练 `COACH_SYSTEM`**（`:627-685`，约 60 行中文提示词）：输出
`{nodes[{title(动作句), purpose(日常经验切口), minutes, difficulty(1-5), teaches[{name,tier}],
assumes[...]}], mints[], completed_endpoints[], note}`；内含档位定义、起点纪律（"教材第一章
≠零基础入口"）、节点纪律（单会话、1-2 新概念）、深度水位、数量契约（"把就绪补到 7 个，
缺口 0 输出空 nodes"）、7 条提交前自查。上下文由 `CoachContext::render`（`:717-789`）拼装：
学习目标/范围参考/罗盘（首批降权省略）/首级判据/就绪读数/花名册/已教概念账/终点锚/覆盖
读数/行为摘要/登记表切片。

**概念评审 `CONCEPT_REVIEW_SYSTEM`**（`:1077-1096`）：输出
`{verdict: pass|reject, problems[{node,field,issue,advice}] ≤8}`；判据 = 挂号充分/前提足够/
命名与内涵一致。失败降级为 PASS（`concept_review`，`:1101-1140`）。

**罗盘 `COMPASS_SYSTEM`**（`:577-588`）：输出 markdown（无代码围栏、无 # 标题），每终点
一节，条目格式 `- **阶段名**：…（深度：知道|会用|能教）（切入：…）（台阶：…）`，双视角
枚举，≤6000 字。

**传统课程对照**（`generation/mod.rs`）：`BLUEPRINT_SYSTEM`（`:31-62`）1 次调用（+1 次纠错
重试）输出结构 JSON；`LESSON_SYSTEM`（`:70-119`）按需逐课时生成 9 种题型活动。传统流程
**没有终点/多方向选择步**。

## 七、i18n 键（zh-CN/learning.json，反推交互步骤）

建课/学习图相关键（`ui/src/renderer/services/i18n/locales/zh-CN/learning.json:349-404`）：
- 向导步进：`learningGraphProposeNext`="下一步：AI 提议终点"（:389）→
  `learningGraphProposeBack`="返回修改目标"（:390）→
  `learningGraphCreateSubmit`="创建课程并开始生长"（:391）
- 第 1 步：`learningGraphGoalInputLabel/Placeholder`（:392-393）、`learningGraphCreateHint`（:394）
- 第 2 步：`learningGraphEndpointStepHint`="以下是 AI 依据学习目标提议的终点锚，可直接修改、
  删除或再添加"（:365）、`learningGraphEndpointTitlePlaceholder`（:360）、
  `learningGraphEndpointNotePlaceholder`（:362）、`learningGraphEndpointRequired`="至少保留
  一条终点"（:364）
- 工作区：`learningGraphEndpointsTitle`="终点"（:354）、`learningGraphCompassTitle`="罗盘"
  （:366）、`learningGraphGrow`="补充节点（就绪 {{ready}}/{{target}}）"（:369）、三档位
  `learningGraphTier_know/apply/teach`="知道/会用/能教"（:386-388）、
  `learningGraphRoleTeaches/Assumes`="教·{{tier}}/假定·{{tier}}"（:384-385）

## 八、对"吸收 learnhub 知识结构生成 + 可视化方案"的关键事实

1. **"多终点选择"已经存在**：学习图向导第 2 步 + `learning_course_endpoints` 落库 + 罗盘
   分节 + 完成裁决。改造落点就在这里；罗盘（`compass_md`）是现下最接近"知识结构"的产物
   但**没有结构化知识图谱数据**。
2. **可视化是最大缺口**：完全无图渲染（无 canvas/布局算法/第三方图库），只有 chips + 列表 +
   表格 + `<pre>` 罗盘。概念网关系数据（`learning_lesson_concepts` + registry）足以支撑
   节点-边渲染，但**没有任何接口输出"边"给前端**——图投影 API 属净新增。
3. **知识结构生成的现状**：生成即"教练批"（节点 + 铸名），概念登记表是副产物而非先行的
   知识结构。
4. 未提交改动（`git diff`）已包含入口域盘点、行为摘要、R 软闸、深度水位等 growth governance
   增量，吸收新方案前需注意**工作区不是干净基线**。
