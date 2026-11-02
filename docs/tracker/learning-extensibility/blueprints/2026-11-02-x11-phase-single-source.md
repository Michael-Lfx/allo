# 施工图 · x11 phase 单源 + pipeline 显式字段

## 1. 元信息

- **目标**：WS 进度帧 `phase` 取值集收成 Rust 单源常量（并给 x12 导出机器可读清单）；课程流帧与 `CourseSummary` 显式携带 `pipeline: sampling|source_plan`；前端删 `isModernPipeline` 嗅探改字段驱动，phase 处理单文件兜底
- **版本**：v1
- **状态**：定稿（用户确认 2026-11-02；施工按本图，变更走 §8 记录）
- **日期**：2026-11-02
- **覆盖票**：x11；图 7（x12 互钉脚本）消费本图 4.4 的导出契约

## 2. 现状摘要

- **发射面**：`events.rs`（58 行）只有事件名字面量（:49/:55）与 7 值 phase 注释（:46）；`scope` 全仓零发射。生产发射点全集（2026-11-02 图 6 读码底稿 A2）：generate.rs 12 处（started/failed×4/publishing/completed/triage_started/triage_completed/source_plan_started/source_plan_ready/coverage_ready）；learning_graph.rs 21 处经 `emit_graph_event`（:4170-4177 注入 `course_id`+`kind:"learning_graph"`+`event` 细分；phase=started/completed/failed/cancelled/basis_declared/basis_revoked）；lesson.rs 10 处（resumed/retry/grounding/started/completed/failed/degraded）；agent 循环翻译层 `learning_loop.rs:290-335`（round + 带 phase 的 loop 事件透传）、`course_outline_loop.rs:279/:392` 与 `lesson_content_loop.rs:481/:673`（generate/repair/audit）。
- **pipeline 门**：`generate.rs:151-154` = `brief.knowledge_base.is_some() && course_outline_engine.is_some() && !kill_switch(:54-58)`；`kind()` = description|knowledge_base（course_outline/mod.rs:111-117）。DTO 现无任何 pipeline 字段；唯一代理信号是 `CourseSummary.source_plan` 是否 None。
- **前端**：`ipcBridge.ts:1778-1878` 手抄 union——**后端发了没声明**：cancelled、basis_declared、basis_revoked、generate、repair（课程流），resumed、retry、degraded、generate、repair（课时流）；**声明了没发**：scope。`model.ts:121-145 deriveStep`、`:192-196 isModernPipeline`（嗅探 `triage_*`）、`:200-218 deriveModernStep`；`CourseGenerationProgress.tsx:25-35/:95-99` 两个可见相集合；`LessonStudy.tsx:725-741`；`CourseGenerationPill` 无 phase；companion `project.ts:93-98` learningPhase 未知→progress，`:100-119` 按 phase 拼宠物 i18n key（nomi.json:665-679 只覆盖 7+5 个旧值）。
- **WS 链路**：service `emit_course_event/emit_lesson_event`（service/mod.rs:248-260）→ `LearningEventEmitter.broadcast`（events.rs:31-43）→ `WebSocketMessage{name,data}` → bus/forward → `wsEmitter`（httpBridge.ts:1327）→ `ipcBridge.learning.*`（:1875-1878）。payload 是调用方手拼 `serde_json::Value`。
- **测试锚点**：tests.rs:5535（grounding phase 断言）、:9576（kb 流 phase 顺序）、:9635（fallback 无 coverage）、:9878（kill switch 回退）；AG learning_loop.rs:796/820/853、lesson_content_loop.rs:2526（repair phase）；FE model.test.ts:115-195。

## 3. 总体结构

```mermaid
graph LR
  consts[events.rs phase 常量单源\n+ manifest 导出测试] --> emit[全部发射点改引常量]
  gate[generate.rs:151 pipeline 门] --> frames[课程流 12 帧显式带 pipeline]
  frames --> dto[CourseSummary.pipeline\ngraph=None / 其余 source_plan|sampling]
  dto & frames --> fe[phaseMeta.ts 前端单源\ndeclared 优先、嗅探兜底、未知忽略]
```

- **全局不变量与禁区**：
  - 事件名 `learning.course-generation` / `learning.lesson-generation` 不变；payload 只**增** `pipeline` 字段（旧客户端忽略未知字段，契约既定）。
  - **不建 DB、不改路由、不改 generation_tasks 状态机**（`set_step` 的 triage/source_plan/expansion 是任务读数，与 WS phase 是两个词表，本票不合并）。
  - graph 流（kind=learning_graph）**不带 pipeline**（其步骤条不区分 v1/v2）；课时流不带（课时按需生成与管线代际无关）。
  - phase 前端字面量只允许出现在 `phaseMeta.ts` 一处（ipcBridge union 放开为 string）。

## 4. 分块图纸

### 4.1 · Rust 常量单源（events.rs）

- **接口契约**：

```rust
pub const EVENT_COURSE_GENERATION: &str = "learning.course-generation";
pub const EVENT_LESSON_GENERATION: &str = "learning.lesson-generation";

pub mod phase {
    // 核心：started / completed / failed / cancelled
    // 课程流：publishing / triage_started / triage_completed /
    //         source_plan_started / source_plan_ready / coverage_ready /
    //         basis_declared / basis_revoked
    // 课时流：resumed / retry / grounding / degraded
    // 循环帧：round / audit / generate / repair
    pub const CORE: &[&str] = &[…]; pub const COURSE: &[&str] = &[…];
    pub const LESSON: &[&str] = &[…]; pub const LOOP: &[&str] = &[…];
    pub const ALL: &[&str] = &[…];   // 四组去重拼接，19 值
}
```

- **调用点改造**：底稿 A2 清单逐处把 `"phase": "started"` 类字面量换 `phase::STARTED` 类常量；AG 翻译层的 generate/repair/audit/round 同（course_outline_loop.rs:279/:392、lesson_content_loop.rs:481/:673、learning_loop.rs:290/:306）。`emit_graph_event` 与 `events.rs:49/:55` 改用事件名常量。
- **失败路径**：无行为变化——纯字面量→常量替换，diff 必须零语义（review 时逐处核对字符串值）。

### 4.2 · pipeline 字段

- **接口契约**：

```rust
// generate.rs
fn course_pipeline(&self, brief: &OutlineBrief) -> &'static str
//   = if brief.knowledge_base.is_some() && engine.is_some() && !disabled() { "source_plan" } else { "sampling" }
//   常量 phase 文案与 COURSE_SOURCE_PLAN_* 同源；门判定与 :151-154 同一函数提取，禁止第二份
```

- `generate_course` 在门判定处算一次 `pipeline`，作为参数传入 `discover_kb_source_plan` / `expand_kb_course_blueprint`，12 个 `json!` 帧各带 `"pipeline": pipeline`。
- **DTO**：`CourseSummary` 增 `#[serde(skip_serializing_if = "Option::is_none")] pipeline: Option<&'static str>`；`course_summary_from_row`：`course_kind == learning_graph` → `None`；否则 `Some(if course_source_plan_json 列非 NULL { "source_plan" } else { "sampling" })`。
- **失败路径**：老行 `course_source_plan_json=NULL` 的知识库课（理论不存在——v2 门失败时四件套缺一即 None）→ 显示为 sampling 步骤条，与今日嗅探结果一致，不新增迁移。

### 4.3 · 前端单源（新文件 `pages/learning/phaseMeta.ts`）

- **接口契约**：

```ts
export const COURSE_PHASES = ['started','publishing','triage_started',… ] as const;   // 与 4.1 COURSE+LOOP 对齐
export const LESSON_PHASES = ['started','resumed','retry','grounding',…] as const;
export const isKnownPhase = (phases: readonly string[], p: string) => phases.includes(p);
// MODERN_VISIBLE_PHASES / LEGACY_VISIBLE_PHASES 由本文件导出（CourseGenerationProgress 改引）
```

- `ipcBridge.ts`：两接口 `phase: string`（union 删除，注释指向 phaseMeta）；`ILearningCourseGenerationEvent` 增 `pipeline?: 'sampling' | 'source_plan'`。
- `model.ts:192-196` 改：

```ts
export function isModernPipeline(events, declared?: 'sampling' | 'source_plan'): boolean {
  if (declared === 'source_plan') return true;
  if (declared === 'sampling') return false;
  return events.some(e => e.phase === 'triage_started' || e.phase === 'triage_completed'); // 兜底
}
```

- `CourseGenerationProgress.tsx`：新增 prop `pipeline?: 'sampling'|'source_plan'`（CourseWorkspace 从 `course.summary.pipeline` 传入）；`:86-88` 改 `isModernPipeline(events, pipeline)`；可见相集合改引 phaseMeta。`LessonStudy.tsx`/`deriveStep`/`deriveModernStep` 字面量改引 phaseMeta；未知 phase 天然落入现有 `else → null` 分支（忽略），不新增分支。
- **失败路径**：后端未来新增 phase → 前端未知值走忽略/中性文案，步骤条不崩（互钉脚本会在 CI 红提示补表——图 7 的职责）。

### 4.4 · x12 导出契约（本图定义，图 7 消费）

```rust
// events.rs #[test] fn export_phase_manifest()
// 每次 cargo test 把 target/learning-phase-manifest.json 写为：
// { "events": {"course": […], "lesson": […]},
//   "phases": {"core": […], "course": […], "lesson": […], "loop": […], "all": […]} }
```

## 5. 决策卡片

### A1 · 常量放 events.rs 分组切片，不做 enum

- **类型**：数据结构
- **决策**：`&'static str` 常量 + 分组 `&[&str]` 切片，不引入 `enum Phase`。
- **理由**：payload 是手拼 `serde_json::Value`（现状摘要），enum 化要改全部 40+ 发射点的构造方式且换不来类型安全（帧字段仍是 Value）；字符串常量 + 分组清单让 x12 互钉脚本可直接消费，改动面最小。
- **代价**：拼错常量名编译期可抓，但「某帧忘带 phase」仍靠底稿 A2 清单逐处核对（施工步验收兜底）。
- **推翻条件**：帧构造全面结构化（payload 结构体化）时，常量升级为 enum 的 `as_str`。

### A2 · pipeline 用字段显式声明，嗅探降级为兜底

- **类型**：接口契约
- **决策**：帧与 DTO 都带 `pipeline`；前端 `declared` 优先、无字段时退回今日嗅探。
- **理由**：嗅探 `triage_*` 在「kb 流但 kill switch 回退」时误判为 modern（回退流不发 triage 帧——tests.rs:9878 已证），字段是唯一无歧义源；保留兜底使滚动部署期（旧帧混入）不炸。
- **代价**：DTO 加一字段、12 个发射点多一参。
- **推翻条件**：v1 管线退役（description 流下线）后删兜底分支。

### A3 · graph/课时流不带 pipeline

- **类型**：模块边界
- **决策**：`pipeline` 只出现在课程流帧与 `CourseSummary`；graph 流、课时流零改动。
- **理由**：学习图生成与课时按需生成不存在 v1/v2 双轨（ADR-0014 只作用于建课管线）；给不区分代际的流标代际是制造假信息。
- **代价**：前端类型上 `pipeline` 是可选项（三处消费点要处理 undefined）——已由 A2 兜底分支覆盖。
- **推翻条件**：学习图或课时生成出现第二条管线。

### A4 · 宠物消息的新 phase 文案用别名表，不扩 nomi.json 键

- **类型**：接口契约（低影响，进未决亦 可）
- **决策**：`project.ts` 的 `titleKey` 查表前先过一张别名映射（`triage_started→started`、`resumed→started`、`degraded→failed`、`basis_declared→completed` 等），nomi.json 只维护既有 12 个键。
- **理由**：宠物气泡是氛围文案不是状态机，12 键覆盖四种情绪足够；为 19 个 phase 各配双语气泡是纯 i18n 税。
- **代价**：别名表是新手工面（与 phaseMeta 同文件维护）。
- **推翻条件**：产品要求某新 phase 有专属气泡文案。

## 6. 施工顺序

| # | 施工步 | 图纸位置 | 验收 |
|---|---|---|---|
| 1 | events.rs 常量 + 事件名常量 + manifest 导出测试 | 4.1/4.4 | `target/learning-phase-manifest.json` 生成且 19 值齐全；`cargo test -p nomifun-learning` |
| 2 | 全部发射点换常量（底稿 A2 清单 43 处） | 4.1 | diff 零字符串值变化（逐处 review）；既有 phase 断言测试（tests.rs:5535/9576/9635/9878、AG 三件）全绿 |
| 3 | generate.rs pipeline 门提取 + 12 帧带字段 + DTO | 4.2 | 新测试：kb 流帧 `pipeline=="source_plan"`、description 流 `=="sampling"`、kill switch 回退 `=="sampling"`；`CourseSummary.pipeline` 三态（graph=None） |
| 4 | 前端 phaseMeta.ts + ipcBridge 放开 + model/组件改造 | 4.3 | `bun run typecheck`；model.test.ts 增 declared 三态用例；`rg "'triage_started'|'source_plan_ready'" ui/src` 仅剩 phaseMeta.ts |
| 5 | 别名表 + 宠物键核对 | A4 | companion 手工过：kb 建课/描述建课/图课程三旅程气泡正常 |

## 7. 未决事项

- **`scope` 死值处置**：TS union 删（本票），Rust 侧本就无。无影响面。推荐默认：随步 4 消失。
- **`basis_declared/basis_revoked` 是否算 course 相**：现归课程流（走 course-generation 通道）。推荐默认：维持通道现状，phaseMeta 归 COURSE 组。触发重议：图课程步骤条要展示声明事件时。

## 8. 变更记录

| 日期 | 类型 | 位置 | 变更 | 根因/触发 |
|---|---|---|---|---|
| 2026-11-02 | 修订 | 2/4.1 | phase 全集从「19 值散三文件」精查为 43 处发射点清单（含 AG 翻译层 generate/repair/audit/round 与 basis_declared/basis_revoked；`scope` 确认死值）；TS 双向漂移清单入档 | 图 6 读码底稿 A2/B1 |
| 2026-11-02 | 修订 | 4.2 | 「kb 流但 kill switch 回退会被嗅探误判」入 A2 理由——tests.rs:9878 证明回退流无 triage 帧 | 读码底稿 C 节 |
