---
id: z03
title: "内容消费面——骨架行声明两行、声明优先条款、scope_findings、旧档负样本"
type: task
status: open
assignee:
blocked_by: [z01]
part_of: ../map.md
---

# z03: 骨架行声明面两行 + scope_findings（报告-only 词面核对）

**What to build:** 学习图节点内容生成的骨架行加「创建意图/教学范围」
两行（缺席省略=旧档零变化）；写作纪律加声明优先条款（两字段在场时是
边界与意图的第一真相）；新模块 `scope_findings`——scope 声明里边界
表述后的名词串机械提取、扫正文节标题与正文、命中逐条 findings
（**报告-only 零拦截零 LLM**，正文否定性提及豁免）；smoke content 臂
判据与播种负样本。仅图路径限域。

## 裁决（推荐项默认裁决——问题未获回答，票面标注可复议）

- **scope_findings 恒开 report-only**（j07 审计观察先例：warning 非阻断、
  findings 是线索不是第二真相）：「不该无痕地生成」由人判读 findings，
  不由机械打回；深层语义越界不设机制面（换说法的越界归真机走查——
  learnhub Q1=b 裁决同律）。零迁移零开关零 LLM。
- **声明优先的 allo 对应物**：teaches_tiers 深度角（D4）与
  degree_statement（结构快照）照旧管「档位/深度」；intent/scope 在场时
  管「意图/边界」且为第一真相——两者是分工不是替代；冲突时 scope 赢
  （范围）而档位标尺仍有效（深度）。

## 现状（2026-11-05 实码核实，基线 939443a1d）

- `service/lesson.rs:1112-1180` `generate_graph_lesson_content`：
  `:1120` SELECT `l.title, l.purpose, l.position, l.estimated_minutes,
  m.title AS module_title`；`:1177-1188` GraphLessonContext 构造
  （goal/degree_statement/prerequisite_path/upcoming_nodes/
  concept_faces/est_minutes/known_names/teaches_tiers/precedents）。
  `service/lesson.rs` 本票独占（z01 不碰；map §文件冲突面）。
- `lesson_draft.rs:223-242` `GraphLessonContext` 结构体；渲染纯函数
  先例 `:59` `render_known_names_line`（空省行）、`:99`
  `render_precedent_block`（空省块）；导出面 `lib.rs:42`。
- 骨架行装配：`nomifun-ai-agent/lesson_content_loop.rs:752-845`
  `lesson_user_text`——图臂「学习图节点」头两行（学习目标/程度声明）
  → 时长预算行（k03）→ 节点（i/N）行+节点定位行（purpose）→
  teaches 档位行（D4 `:804`）→ 前置路径 → 后续节点 → 已知名单行
  （n3 `:827`）→ 先例块（#180 `:833`）→ 概念面（j07）→
  search_concepts 提示（y03 `:840`）。
- findings 通道：`lesson_draft.rs:1290-1380` 审计观察面——图路径概念面
  观察（naming_mixed/naming_canonical_only/first_seen_unintroduced）+
  R9 叙述声部两管线通用（`warning(kind, format!(...))` 形状）；
  warning 进生成报告与观测面，非阻断。
- 传统线：`lesson_content_loop.rs:850` goal 行 + outline_tree 臂——
  传统课时无 scope 概念，本票全部图路径限域（`context.graph.is_some()`
  同款判定先例 `:759`）。
- smoke：`nomifun-app/src/commands/learning_smoke.rs` content 臂
  （`run_content`，`:53`；九判据见 y 波批次——本票追加判据不重构旧判据）。

## learnhub 参考（main@20f1dcf）

- `src-tauri/src/engine/contentgen/scope_findings.rs`（fda5469 新文件，
  321 行）：模块 doc（Q1=b 报告-only 零拦截；否定性提及豁免；深层语义
  越界不设机制面）；`BOUNDARY_MARKERS` 12 词（不教/不展开/不单独/不讲/
  不引入/不涉及/不碰/不深究/留待/归后续/留给后续/后续学习点）；
  `MENTION_NEGATIONS` 21 词（前 12 + 超出本课/超出本点/超出范围/不在本课/
  不在本点/不在范围/不属于本课/不属于本点/不属本课/不属本点）；
  提取面=词表命中处之后至子句界为排除段；扫描面=节标题与正文文本；
  命中逐条 findings（指名来源字段「本点范围」与命中位置「节N 标题/正文」）。
- `src-tauri/src/domain/node_ctx.rs`（fda5469 +80 行）：骨架行声明面
  两行渲染（缺席省略）。
- `src-tauri/src/engine/contentgen/facade.rs`（fda5469 +209 行）：
  声明优先条款装配（声明在场优先于意图画像推断）+ scope_findings 接线。
- 提示词：`prompts/contentgen/content-write.md`（fda5469 +4 行——
  「点名边界不构成引用」配对条款；边界诚实写作条款）。
- `3b07214` smoke-content 基线：九判据全过 + **播种旧档负样本**
  （无 scope 旧行不渲染不报）。
- `b5d433f` 词条「教学范围」：范围外点名（否定性提及）不构成引用。

## 实施要点

### A. 上下文贯通（service/lesson.rs + lesson_draft.rs）

- [ ] `generate_graph_lesson_content:1120` SELECT + `l.intent, l.scope`；
  GraphLessonContext 构造补两 `Option<String>`（空串→None 折叠）。
- [ ] `lesson_draft.rs` GraphLessonContext + `intent: Option<String>` +
  `scope: Option<String>`（serde 不涉及——进程内结构）。
- [ ] 单节重写路径共用 graph_node_topology/context 构造——确认重写臂
  同享两字段（`generate_graph_lesson_content` 与单节重写的公共装配点，
  实施时核对重写入口是否复用本 SELECT）。

### B. 骨架行两行 + 声明优先（lesson_content_loop.rs + 渲染纯函数）

- [ ] `lesson_draft.rs` 新纯函数
  `render_intent_scope_lines(intent: Option<&str>, scope: Option<&str>)
  -> Option<String>`（None/空 = 整段省略；有任一 = 只渲染在场行）；
  经 `lib.rs` 导出（render_known_names_line 同款）。
- [ ] `lesson_content_loop.rs` lesson_user_text 图臂：节点定位行
  （purpose）后插两行——措辞对齐骨架行现有风格（建议：
  「创建意图：{intent}\n教学范围：{scope}（范围外内容不教，越界点名
  不构成引用）」；缺席省略保留旧行为）。
- [ ] 声明优先条款进图臂写作纪律区（nomifun-learning 侧提示词资产或
  content-write 对应块——落点实施时按现有内容纪律块的归属定）：
  intent/scope 在场时为本节点意图与边界的**第一真相**；teaches_tiers
  深度角与 degree_statement 照旧管档位/深度；边界诚实条款（范围内
  讲透、范围外不无痕展开；小结/引入处诚实写边界合法——否定性提及
  不算越界）。
- [ ] 两臂单测：两字段齐→两行在场；单缺席→单行；全空→整段省略
  （零变化）。

### C. scope_findings 新模块（nomifun-learning，图路径限域）

- [ ] 新文件 `learning_graph/scope_findings.rs`（或 generation/ 侧——
  与消费点 lesson_draft.rs 同 crate 即可，落点实施时按模块归属定）：
  - `BOUNDARY_MARKERS`（12 词照抄 learnhub，域中立）与
    `MENTION_NEGATIONS`（21 词）两个 pub(crate) 常量。
  - 提取：scope 文本中边界词命中处之后至子句界（句读/分号/句号）为
    排除段，段内名词串提取（复用 `definition_audit::grounding_runs`
    切片或更简单的连续 CJK 串——learnhub 用词表后切片，实施时以
    learnhub 实码为准逐行对照）。
  - 扫描：节标题与正文文本逐段；命中即一条 finding（指名来源字段
    「教学范围」与命中位置「节 N 标题/正文」）；命中位置所在子句含
    任一 MENTION_NEGATIONS 词 = 豁免不报。
  - 输出形状 = 既有 `warning(kind, format!)` findings 通道同形
    （kind 如 `scope_boundary_hit`），进 `lesson_draft.rs:1290`
    审计观察面——图路径分支内接线（`context.graph` 持有 scope 时才
    跑；None 零开销）。
- [ ] 报告-only 断言：findings 只进观测面/生成报告，不参与门禁与重试
  （j07 同律，测试钉住「有 findings 仍发布成功」）。
- [ ] 单测：播种负样本（scope「不引入方程」+ 正文节标题含「方程」→
  命中 1 条）；否定性提及豁免（正文「本课不引入方程」→ 0 条）；
  旧档（scope None → 零扫描零 finding）；空 scope → 零扫描。

### D. smoke（learning_smoke.rs content 臂）

- [ ] content 臂判据追加：①骨架行两行渲染断言（观测档案里 prompt
  含「创建意图/教学范围」行——ArchivingCompleter 档案可查）；
  ②播种旧档负样本（learnhub `3b07214` 同款：生成后将任一节点行
  intent/scope 置空再走一次装配→两行省略、scope_findings 零触发）；
  ③scope_findings 读数行（报告-only：hits 计数打印，非门禁）。
- [ ] 无凭据臂照旧如实非零失败。

## Non-goals

- 不做传统线的范围核对（传统课时无 scope 声明面——骨架行走 outline
  臂，本票不动）。
- 不做深层语义越界的机制面（换说法越界归真机走查；learnhub Q1=b 同律）。
- 不做 scope 的用户编辑面与「超载问题信号」类联动（learnhub #194 的
  capacity_note 联动已在前波裁决范围外）。
- 不动评审面（z02 领域）；不碰 `service/course.rs` 与
  `service/learning_graph.rs`（z01/z02 领域——冲突面已划清）。

## 验收

1. `cargo test -p nomifun-learning -p nomifun-ai-agent` 全绿，新测全过
   （播种负样本/豁免/旧档三态必在场）。
2. 真机 content 臂 EXIT=0：判据①②③在场；两行渲染可见。
3. 旧档课程（存量开发库）内容生成零两行、零 scope findings、行为与
   基线一致（旧档零变化）。
4. report-only 断言测试在场（有 findings 仍发布成功）。

## 勘误（双轴评审修复轮注记）

- 票面写 `MENTION_NEGATIONS` 21 词——learnhub 实码为 12 + 10 = 22 词
  （`BOUNDARY_MARKERS` 12 + 否定性补充措辞 10），实现照实码 22。
- `CLAUSE_DELIMS` 全角补全（，！？）为 upstream 缺陷修正：learnhub
  源文件只剩半角逗/叹/问号的重复条目（全角形丢失的痕迹），allo 正文
  与 scope 是全角中文标点，不补全角形则排除段过滚——已在
  `scope_findings.rs` 模块 doc 注明。
