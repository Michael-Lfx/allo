---
id: z01
title: "节点声明面两字段——契约门、生长提示词、落库、DTO、UI"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# z01: 节点声明面两字段（intent/scope）——底座贯通

**What to build:** 生长批节点新增两个出生即声明的字段并贯通 allo 全链：
`intent`（为何有这个学习点、课里承担什么角色，一句话）与 `scope`（教什么/
教到哪/明确不碰什么，一两句），各 ≤200 字符。契约门强制必填（`ng.node.brief`），
生长提示词教写法，落 `learning_lessons` 两列（空=旧档），DTO/TS 镜像透出，
课时详情页两行展示（缺席不渲染）。

## 裁决（推荐项默认裁决——问题未获回答，票面标注可复议）

- **purpose 保留不动，另加 intent/scope**：purpose=学完能做什么（outcome
  面），intent=为何有此点（role 面），scope=边界声明。三者语义分立。
  复议点：若用户裁决 intent 吸收 purpose，则 purpose 退役改名，消费点
  全部换键——工作量显著放大，需重排本波。
- **空串 = 旧档**（非 Option 列）：跟随 purpose/difficulty 列先例
  （`NOT NULL DEFAULT ''`），serde 侧 Option 读侧折叠为空串语义。

## 现状（2026-11-05 实码核实，基线 939443a1d）

- `growth_contract.rs:20`：`NODE_KEYS = [title, purpose, est, difficulty,
  teaches, assumes]`；条款表 doc header 在文件头部（learnhub validator.rs
  条款表同构位）。
- `learning_graph/mod.rs:168-186`：`ProposedNode`，derive
  `PartialEq, Eq, Serialize, Deserialize`；`growth_retry.rs:177`
  `redrafted != node` 用 derived PartialEq——新字段自动进点修守卫
  （learnhub「未点名条目整对象机械比对自动含两字段」同效，免费拿到）。
- 生长提示词：`mod.rs:488` 节点 JSON 形状示例含 `"purpose"`；
  `:513-550` 节点纪律区；`:723` 归一化 `node.purpose = node.purpose.trim()`
  同款位。
- 落库：`service/learning_graph.rs:3486` apply_batch
  `INSERT INTO learning_lessons (lesson_id, module_id, title, summary,
  purpose, position, estimated_minutes, difficulty, content_generated)`。
- 列现状：015 建表（title/summary/position/estimated_minutes/source_*）、
  040 `ADD COLUMN purpose TEXT NOT NULL DEFAULT ''`、070 `:271-274`
  `ADD COLUMN difficulty INTEGER NOT NULL DEFAULT 2`（070 仍未发布，
  PR#350 对 main CONFLICTING）。
- DTO：`models.rs:2653` LessonView / `:345` LessonPack（est serde
  alias 双读先例在 `:352` 附近）；读出 `service/course.rs:346/:492`
  SELECT → `:529/:384` LessonView 构造。
- UI：`ui/src/renderer/pages/learning/types.ts:311` Lesson 接口；
  `LessonStudy.tsx:791-792` purpose 段（`lesson.purpose &&` 条件渲染
  先例就在眼前）。
- smoke：`nomifun-app/src/commands/learning_smoke.rs` grow 臂
  （`run_grow`，`:128/:135`）。

## learnhub 参考（main@20f1dcf）

- `src-tauri/src/engine/node_growth/validator.rs:335-460`：
  `ng.node.brief(#205 v1.20)`——intent/scope 必填非空、各 ≤200 字符
  （`NODE_BRIEF_MAX_CHARS`）；`NODE_KEYS` 扩到八键；违约消息带写法指引。
- `src-tauri/src/engine/node_growth/placement.rs:149-163`：LearningNode
  两字段 Option + serde default + skip_serializing_if（缺席不落盘）；
  `:273-300` roundtrip 与旧档读测试（est/batch 先例同款）。
- `docs/contracts/node-growth-artifacts.md`（v1.20 条款表）与
  `prompts/node-growth/node-growth.md`（两字段要求与写法条款——一句话
  意图/一两句范围；「不教 X」是边界声明）。
- GUI：`43ed292` 详情页「为什么学/学到哪」两行（缺席整段不渲染，
  存量零变化）。

## 实施要点

### A. 契约门（growth_contract.rs）

- [ ] `NODE_KEYS` + `"intent"`, `"scope"` 两键；条款表 header 增
  `ng.node.brief` 行（措辞对齐 learnhub：非空字符串、各 ≤200 字符、
  一句话意图/一两句范围、进评审机械点名扫描——后半句 z02 落地）。
- [ ] 新条款检查：逐节点 intent/scope 非空（trim 后）且
  `chars().count() <= 200`；违约消息照 learnhub 三态（缺席/空白/超限）
  各带指引。常量 `NODE_BRIEF_MAX_CHARS: usize = 200` pub。
- [ ] 单测：缺席违约/空白违约/恰好 200 过/201 拒/两键齐过。

### B. 生成结构（learning_graph/mod.rs + growth_contract.rs）

- [ ] `ProposedNode` + `#[serde(default)] pub intent: String` +
  `#[serde(default)] pub scope: String`（空串容旧——生长 LLM 输出经
  容错反序列化进本结构，门在 contract 层拦；与 learnhub Option 不同形
  但语义同：allo 侧空串即缺席）。
- [ ] 生长提示词 `:488` 形状示例 + 两键；`:513-550` 纪律区加条款：
  intent 一句话（为何有这个点、课里承担什么角色）、scope 一两句
  （教什么/教到哪/明确不碰什么；「不教 X」是边界声明不是内容）；
  拆分产物的两字段必须逐半重写（照各自那一半的实际内容，禁止照抄
  原条目——learnhub 拆并出并病历条款的前半，软条款）。
- [ ] 归一化 `:723` 附近补 `intent/scope` trim 同款。
- [ ] `growth_retry.rs:146` 守卫 doc 注释补两字段名（行为零改，
  derived PartialEq 已覆盖）。
- [ ] 提示词资产三闸（金样/围栏/条款覆盖断言）随批——沿 j08 流程；
  本票条款在生长提示词内联（allo 生长提示词未资产化，内联即可，
  断言落单测 substring）。

### C. 落库（迁移 070 + apply_batch）

- [ ] `070_learning_domain_rebuild.sql` 「原 073_learning_lesson_difficulty」
  节同区追加：
  `ALTER TABLE learning_lessons ADD COLUMN intent TEXT NOT NULL DEFAULT '';`
  `ALTER TABLE learning_lessons ADD COLUMN scope TEXT NOT NULL DEFAULT '';`
  迁移测试同步（列序/缺省；v3 契约无新表无新 _id，PRODUCT_TABLES 不动；
  schema 测试 grep 禁词避让照 j09 教训）。
- [ ] `service/learning_graph.rs:3486` INSERT 列表 + intent, scope，
  bind `node.intent.trim()` / `node.scope.trim()`。

### D. DTO 读出（models.rs + course.rs）

- [ ] `LessonView` + `pub intent: String` + `pub scope: String`
  （文档注释：空=旧档；1..=200 字符约束在生成侧，读侧不重验）。
- [ ] `LessonPack` + 两键 `#[serde(default)]`（旧导入包容缺；不做
  alias——learnhub 导入包键名即 intent/scope）。
- [ ] `service/course.rs:346/:492` SELECT + `l.intent, l.scope`；
  `:529/:384` 构造补两字段。

### E. UI（types.ts + LessonStudy + i18n）

- [ ] `types.ts` Lesson + `intent: string` + `scope: string`
  （注释：空=旧档不渲染）。
- [ ] `LessonStudy.tsx:791` purpose 段后插两行，`lesson.intent &&` /
  `lesson.scope &&` 条件渲染（purpose 同款）；标签走 i18n
  （「创建意图」「教学范围」双语键；主题 token 用现有
  `text-t-secondary` 同款）；样式对齐 purpose 段。
- [ ] `bun run gen:i18n` → `check:i18n` → `check:theme` → `check:dead-css`。

### F. smoke（learning_smoke.rs grow 臂）

- [ ] grow 臂判据追加：①首批契约门过（无 ng.node.brief 违约读数）；
  ②落库读回首批每行 intent/scope 非空；③读数行打印
  `brief: intent_avg=N/scope_avg=M` 一类（读数面非门禁）。
- [ ] 无凭据臂照旧如实非零失败。

## Non-goals

- 不动评审侧（点名四源/范围外裁决归 z02）。
- 不动内容消费面（骨架行/scope_findings 归 z03）。
- 不做 intent/scope 的编辑面（用户改两字段的 UI/API 后置——learnhub
  也未做，GUI 只读两行）。
- 不给 graph_node_topology 前置/后续/禁止清单摘要加 scope（map 雾区）。

## 验收

1. `cargo test -p nomifun-learning -p nomifun-db`（db 单线程）全绿，
   新测全过；`cargo test -p nomifun-app` smoke 相关单测过。
2. `bun run typecheck` = 55 存量（学习 0）；i18n/theme/dead-css 绿。
3. 真机 grow 臂 EXIT=0：判据①②③在场。
4. 旧档课程（存量开发库）课时详情零渲染两行、零报错（旧档容忍）。
