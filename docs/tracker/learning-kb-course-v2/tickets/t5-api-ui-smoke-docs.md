---
id: t5
title: "DTO/WS/UI/冒烟/域文档：清点分诊与覆盖报告可见"
type: task
status: open
assignee:
blocked_by: [t4]
part_of: ../map.md
---

# t5: API/UI/冒烟/文档收口

**What to build:** 把 t1–t3 的清点、分诊、结构计划、覆盖报告通过 DTO/WS/UI 暴露出来，并让 `learning-smoke course` 能单独跑三库（clean/dirty/large）冒烟。目标是“跳过与不确定可见”，不是再做一个后台黑盒。

**Blocked by:** [t4 持久化](t4-persistence-and-migration.md)。

**Status:** ready-for-agent

## Grill 第一轮/第二轮定案（2026-10-21）

- **术语**：UI/文案统一用 `SourcePlan`（来源结构计划）与 `SourceCoverage`（来源覆盖）；`Structure`/`Coverage Gauge` 只指学习图，禁止混用。
- **单位**：展示“文件级角色 + 章节/主题级覆盖”；覆盖卡以 topic 为主，文件列表作诊断；`mixed` 文件显示“部分内容进入课程”，不是整文件跳过。
- **失败策略**：只有零章节/零 core 才硬失败；部分覆盖缺口建课 + 醒目 warning；`degraded`/`fallback`/`anchor_uncertain` 必须有可见文案。
- **成本门**：>40 文件或预估 >20 次 LLM 调用时，建课前展示估算并确认；低于阈值自动跑；进度条展示 triage/source_plan/expansion 阶段。
- **持久化**：读 `course_source_plan_json` 单列快照；未知 version 显示“不可用”，不崩溃；旧课程 null 不显示空卡。
- **发布/审阅（Q13）**：不加独立“生成前审阅”页；大库成本确认弹窗里顺带展示 triage 计数/跳过摘要；生成中/详情看完整报告；内部 kill switch/legacy fallback；不做用户可见 beta flag。
- **生长复用**：A/E/F 已并入本波；B/C/D 等 `learning-goal-kb/t5`，本票不做图课程 UI/KB 入口。

## 后端 DTO / 事件

- [ ] `CourseSummary` / `CourseDetail` 增加可选 `source_plan` 视图（旧课程/描述流为 null）：
  - `triage`: `{ total_files, core, prerequisite, practice, mixed, noise, duplicate, uncertain, degraded, warnings[] }`
  - `plan`: `{ chapters, concepts, edges, gaps[], warnings[] }` + 章节/来源映射摘要
  - `coverage`: `{ topics_covered, topics_skipped, prerequisite_topics, practice_topics, anchor_uncertain_topics, uncertain[], skipped_examples[] (≤20), fallback_lessons, warnings[] }`；文件级 role 只作诊断列表
  - `duplicates`: `{ count, examples[] (≤20) }`（path + duplicate_of + canonical_reason）
  - `snapshot`: `{ version, available }`（未知版本 `available=false`）
- [ ] 详情端点保留完整 `excluded`（path + role + reason，可分页/限长）与 `duplicates`；列表端点只给计数/示例，避免拖慢。
- [ ] WS `learning.course-generation` 增加阶段：
  - `triage_started` / `triage_completed`（counts/degraded）
  - `source_plan_started` / `source_plan_ready`（chapters/concepts/gaps）
  - `coverage_ready`（topics_covered / skipped / fallback）
  - 旧客户端忽略未知 phase；`started/completed/failed` 不变。
- [ ] 生成任务注册表（`generation_tasks`）的 step 文案覆盖 triage/source_plan/expansion；取消/失败语义沿用现状。

## 前端

- [ ] 建课对话框/悬浮进度：显示“扫描 N 文件 → core X / mixed Y / 跳过 Z / 重复 W / 不确定 U”；可展开查看跳过理由、duplicate_of、uncertain 列表；`degraded` 时明确提示“规则分诊，未做 LLM 判定”。
- [ ] 成本门：>40 文件或预估 >20 次 LLM 调用时，提交前显示估算（文件数/批数/大概调用数/预计耗时）与 triage 计数/跳过摘要 + “继续/取消”；低于阈值直接跑。
- [ ] 课程详情：新增“来源覆盖”卡（topic 覆盖、mixed 的进入/跳过段、prerequisite/practice 去向、duplicate canonical、anchor_uncertain、fallback 课、warnings；文件级 role 作诊断列表）；旧课程不显示空卡。
- [ ] 章节/课时来源展示：沿用现有 source 展示；新增 `anchor`/`source_chapters`/`anchor_uncertain` 文案（若 t3 落库）。
- [ ] i18n：zh-CN/en-US 双语对称；所有用户可见文案走 i18n；加双语言断言测试。
- [ ] 空态/错误态：无 plan（旧课程/未知 version/描述流）、全 noise（无课可建）、uncertain 过多、LLM 降级——分别有文案，不静默。
- [ ] 不改现有建课流程的默认行为：旧客户端/描述流无 `source_plan` 时 UI 不报错。

## 冒烟

- [ ] `learning-smoke course` 增加：
  - `--triage-only`：只跑清点+分诊，打印 counts/excluded reasons/uncertain，退出码 0（有 warning 也 0）；
  - `--plan-only`：分诊+结构计划，打印 chapters/coverage/gaps；
  - `--lesson N` 维持现有课时正文抽样；
  - `--report-json <path>`（可选）：把 triage/plan/coverage 落 JSON 便于对账。
- [ ] 三库冒烟：
  1. **clean**：19 文件《数据结构思维》——无回归；
  2. **dirty**：干净教材 + 无关文件/翻译副本/重复/乱序文件名/缺标题/网页推广尾段/一个 mixed 文件——noise/duplicate 不进入课程，core/mixed 的相关 topic 全成课，uncertain 可见；
  3. **large**：105 文件 Rust 书——20 章覆盖或显式 skipped，无未解释倒挂。
- [ ] dirty 样本的建设方式：测试 fixture（临时目录生成）或 `scripts/` 下的本地脚本；不要把脏数据写进生产 mock/seed。

## 文档

- [ ] **ADR-0014**（下一个可用编号，实施时确认）：脏库/大教材的课程结构发现——Inventory/Triage/Evidence Structure/Batched Expansion/覆盖披露的裁决与取舍；引用本 map 与 asset。
- [ ] `CONTEXT.md` 新词条：Kb Inventory / Kb Triage / Source Plan / SourceCoverage / Mixed File / Anchor Uncertain / Uncertain（与现有 Tier/Structure/Coverage Gauge 明确区分；`Structure`/`Coverage Gauge` 不重定义）。
- [ ] `docs/architecture/learning.zh.md` 新节：传统建课的脏库路径、与 t3 课时检索的关系、40 文件预算的新定位（单批展开，不是结构发现）。
- [ ] `docs/contracts/learning-course-source-plan.md`：triage/plan/coverage 的 JSON 形状、CHECK 约束、旧行 NULL 兼容、来源锚点不变量、`mixed`/`duplicate_of`/`anchor_uncertain` 语义、版本号策略。
- [ ] 若 t4 改表/列，更新 `docs/architecture/learning.zh.md` 迁移记录与 v3 schema 说明。

## 验收

- `bun run typecheck` / `bun run check:i18n` / `bun run check:theme` / `bun run check:dead-css` 过；新增文案双语对称。
- DTO 测试：旧课程 null / 新课程完整 / 描述流 null / 未知 version 四态；WS phase 兼容旧客户端。
- `learning-smoke course` 三库跑通：clean EXIT=0；dirty EXIT=0 且 noise/duplicate 有 reason、mixed 有进入/跳过段；large EXIT=0 且 coverage/gaps 可读。
- 成本门：>40 文件或预估 >20 次调用时出现估算/确认（含 triage 摘要）；低于阈值不打扰。
- UI 文案覆盖 mixed / duplicate canonical / anchor_uncertain / fallback / degraded；双语文案对称。
- 域文档/ADR/契约/词条齐全；`bun run check` 相关门过。
- 不在生产 Context/hook 里硬编码任何 triage/plan mock 数据。

## 备注

- UI 只展示快照，不实时重算 KB；KB 变更后课程不自动变（与现有课程快照语义一致）。
- `skipped_examples ≤20` 是为列表端点限长；详情页可看全量（分页）。
- 所有“跳过/降级/fallback”必须在 UI 有可见文案，避免再次出现“静默截断”。
